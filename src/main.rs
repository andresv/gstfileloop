use anyhow::{anyhow, Ok, Result};
use gst::prelude::*;
use gstreamer as gst;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const INPUT_FILE: &str = "/Users/andres/Downloads/2022-03-23--08-17-34/cam_front_left.mp4";
const OUTPUT_FILE: &str = "/Users/andres/Downloads/out.mp4";

/// `qtdemux` has src pads that appear dynamically.
/// This function will be called when `qtdemux` src pad becomes available.
fn demux_pad_added_cb(
    pipeline_weak: gst::glib::WeakRef<gst::Pipeline>,
    src_pad: &gst::Pad,
    pad_index: u32,
    stop: Arc<AtomicBool>,
) {
    println!("qtdemux[{:}] pad added", pad_index);
    match pipeline_weak.upgrade() {
        Some(pipeline) => {
            // create closure for easier error-handling.
            let link_demux = || -> anyhow::Result<()> {
                let concat = pipeline.by_name("concat").unwrap();

                let sink_pad = concat
                    .request_pad_simple("sink_%u")
                    .ok_or_else(|| anyhow!("`concat` does not have `sink_%u` pad"))?;

                src_pad
                    .link(&sink_pad)
                    .map_err(|_| anyhow!("cannot link `demux` to `concat`"))?;
                Ok(())
            };

            let pipeline_weak = pipeline_weak.clone();
            src_pad.add_probe(gst::PadProbeType::DATA_DOWNSTREAM, move |_, probe_info| {
                demux_probe_event_cb(pipeline_weak.clone(), probe_info, pad_index, Arc::clone(&stop))
            });

            if let Err(e) = link_demux() {
                println!("cannot link demux {:?}", e);
            }
        }
        None => return,
    }
}

/// This is called everytime when some event goes through `qtdemux`.
/// EOS means that `filesrc` has finished reading and new filesrc can be prepared.
fn demux_probe_event_cb(
    pipeline_weak: gst::glib::WeakRef<gst::Pipeline>,
    probe_info: &mut gst::PadProbeInfo,
    pad_index: u32,
    stop: Arc<AtomicBool>,
) -> gst::PadProbeReturn {
    match probe_info.data {
        Some(gst::PadProbeData::Event(ref ev)) if ev.type_() == gst::EventType::Eos => {
            if stop.load(Ordering::Acquire) {
                println!("qtdemux[{:}] EOS - STOP!", pad_index);
            } else {
                println!("qtdemux[{:}] EOS", pad_index);

                let pipeline = pipeline_weak.upgrade().expect("cannot get pipeline");
                std::thread::spawn(move || {
                    println!("remove filesrc[{}]->qtdemux[{}]", pad_index, pad_index);

                    let remove = || {
                        let old_filesrc = pipeline.by_name(&format!("filesrc{}", pad_index)).unwrap();
                        let old_qtdemux = pipeline
                            .by_name(&format!("qtdemux{}", pad_index))
                            .ok_or_else(|| anyhow!("cannot get qtdemux"))?;
                        pipeline.unlink(&old_filesrc);
                        pipeline.unlink(&old_qtdemux);

                        old_filesrc.set_state(gst::State::Null)?;
                        old_qtdemux.set_state(gst::State::Null)?;

                        pipeline.remove(&old_qtdemux)?;
                        pipeline.remove(&old_filesrc)?;
                        Ok(())
                    };

                    if let Err(e) = remove() {
                        println!("cannot remove used filesrc and qtdemux{:?}", e);
                    }
                });

                // create closure for easier error-handling.
                let prepare_new_filesrc = || -> anyhow::Result<()> {
                    let pipeline = pipeline_weak.upgrade().ok_or_else(|| anyhow!("cannot get pipeline"))?;

                    let filesrc = gst::ElementFactory::make("filesrc", None)?;
                    filesrc.set_property("location", INPUT_FILE);

                    let qtdemux = gst::ElementFactory::make("qtdemux", None)?;
                    qtdemux.connect_pad_added(move |_dmux, src_pad| {
                        demux_pad_added_cb(pipeline_weak.clone(), src_pad, pad_index + 2, Arc::clone(&stop))
                    });

                    pipeline.add_many(&[&filesrc, &qtdemux])?;
                    gst::Element::link_many(&[&filesrc, &qtdemux])?;

                    filesrc.sync_state_with_parent()?;
                    qtdemux.sync_state_with_parent()?;
                    Ok(())
                };

                if let Err(e) = prepare_new_filesrc() {
                    println!("cannot prepare new filesrc {:?}", e);
                }
            }
        }
        _ => (),
    }
    gst::PadProbeReturn::Ok
}

fn main() -> Result<()> {
    gst::init()?;
    let pipeline = gst::Pipeline::new(None);
    let pipeline_weak = pipeline.downgrade();
    let stop = Arc::new(AtomicBool::new(false));

    for i in 1..3 {
        let filesrc = gst::ElementFactory::make("filesrc", Some(&format!("filesrc{}", i)))?;
        filesrc.set_property("location", INPUT_FILE);
        let demux = gst::ElementFactory::make("qtdemux", Some(&format!("qtdemux{}", i)))?;
        let pw = pipeline_weak.clone();
        let s = Arc::clone(&stop);
        demux.connect_pad_added(move |_dmux, src_pad| demux_pad_added_cb(pw.clone(), src_pad, i, Arc::clone(&s)));

        pipeline.add_many(&[&filesrc, &demux])?;
        gst::Element::link_many(&[&filesrc, &demux])?;
    }

    let concat = gst::ElementFactory::make("concat", Some("concat"))?;
    let parse = gst::ElementFactory::make("h265parse", None)?;
    let mp4mux = gst::ElementFactory::make("mp4mux", None)?;
    let filesink = gst::ElementFactory::make("filesink", None)?;
    filesink.set_property("location", OUTPUT_FILE);
    filesink.set_property("sync", &true);
    filesink.set_property("async", &false);
    pipeline.add_many(&[&concat, &parse, &mp4mux, &filesink])?;
    gst::Element::link_many(&[&concat, &parse, &mp4mux, &filesink])?;

    // log all frames
    let src_pad = mp4mux.static_pad("src").unwrap();
    src_pad.add_probe(gst::PadProbeType::DATA_DOWNSTREAM, move |_, probe_info| {
        match probe_info.data {
            Some(gst::PadProbeData::Event(ref ev)) => {
                println!("event {:?}", ev);
            }
            Some(gst::PadProbeData::Buffer(ref buf)) => {
                println!("buf {:?}", buf);
            }
            _ => (),
        }
        gst::PadProbeReturn::Ok
    });

    let bus = pipeline.bus().unwrap();
    let pipeline = pipeline.dynamic_cast::<gst::Pipeline>().unwrap();
    let ret_pipeline = pipeline.clone();

    let thread = std::thread::spawn(move || {
        let bus = bus;

        for msg in bus.iter_timed_filtered(
            gst::ClockTime::NONE,
            &[
                gst::MessageType::Eos,
                gst::MessageType::Error,
                gst::MessageType::StateChanged,
            ],
        ) {
            use gst::MessageView;

            match msg.view() {
                MessageView::Eos(..) => {
                    println!("received EOS");
                    if let Err(e) = pipeline.set_state(gst::State::Null) {
                        println!("cannot set pipeline to Null {:?}", e);
                    }
                    break;
                }
                MessageView::Error(err) => {
                    println!(
                        "error from {:?}: {} ({:?})",
                        err.src().map(|s| s.path_string()),
                        err.error(),
                        err.debug()
                    );
                    break;
                }
                MessageView::StateChanged(state) => {
                    match msg.src() {
                        Some(_src_obj) => {
                            println!("pipeline state: {:?} -> {:?}", state.old(), state.current());
                        }
                        None => {
                            println!("received state-changed message without valid msg source");
                        }
                    };
                }
                _ => (),
            };
        }

        println!("exit pipeline thread");
    });

    ret_pipeline.set_state(gst::State::Playing).unwrap();
    std::thread::sleep(Duration::from_secs(15));

    println!("send EOS");
    stop.store(true, Ordering::SeqCst);
    ret_pipeline.send_event(gst::event::Eos::new());
    thread.join().unwrap();

    Ok(())
}
