use anyhow::{anyhow, Result};
use gst::prelude::*;
use gstreamer as gst;
use std::time::Duration;

//  gst-launch-1.0 -v filesrc location=cam_front_center.mp4 ! qtdemux ! filesink location=out.mp4 sync=true

const INPUT_FILE: &str = "/Users/andres/Downloads/2022-03-23--08-17-34/cam_front_left.mp4";
const OUTPUT_FILE: &str = "/Users/andres/Downloads/out.mp4";

fn main() -> Result<()> {
    gst::init()?;
    let pipeline = gst::Pipeline::new(None);
    let pipeline_weak = pipeline.downgrade();

    let filesrc = gst::ElementFactory::make("filesrc", None)?;
    filesrc.set_property("name", "filesrc");
    filesrc.set_property("location", INPUT_FILE);

    let demux = gst::ElementFactory::make("qtdemux", None)?;
    demux.set_property("name", "demux");

    pipeline.add_many(&[&filesrc, &demux])?;
    gst::Element::link_many(&[&filesrc, &demux])?;

    demux.connect_pad_added(move |_dmux, src_pad| {
        let pipeline = match pipeline_weak.upgrade() {
            Some(pipeline) => pipeline,
            None => return,
        };

        let pipeline_weak = pipeline_weak.clone();

        let mp4mux = gst::ElementFactory::make("mp4mux", None).unwrap();
        let mp4mux_src_pad = mp4mux.static_pad("src").unwrap();

        mp4mux_src_pad.add_probe(gst::PadProbeType::DATA_DOWNSTREAM, move |_, probe_info| {
            match probe_info.data {
                Some(gst::PadProbeData::Event(ref ev)) => {
                    println!("event {:?}", ev);
                    if ev.type_() == gst::EventType::Eos {
                        println!("mp4mux EOS");

                        // `set_state` cannot be called directly from closure that is why there is thread here
                        let pipeline_weak = pipeline_weak.clone();
                        std::thread::spawn(move || {
                            if let Some(pipeline) = pipeline_weak.upgrade() {
                                let demuxer = pipeline
                                    .by_name("demux")
                                    .expect("cannot get `demux` from pipeline");
                                let filesrc = pipeline
                                    .by_name("filesrc")
                                    .expect("cannot get `filesrc` from pipeline");

                                filesrc
                                    .set_state(gst::State::Null)
                                    .expect("cannot set `filesrc` state to Null");

                                filesrc.unlink(&demuxer);
                                pipeline
                                    .remove(&filesrc)
                                    .expect("cannot remove `filesrc` from pipeline");

                                let filesrc = gst::ElementFactory::make("filesrc", None).unwrap();
                                filesrc.set_property("name", "filesrc");
                                filesrc.set_property("location", INPUT_FILE);
                                pipeline.add(&filesrc).unwrap();
                                filesrc.link(&demuxer).unwrap();

                                filesrc.sync_state_with_parent().unwrap();
                                filesrc.set_state(gst::State::Playing).unwrap();
                            }
                        });

                        return gst::PadProbeReturn::Drop;
                    }
                }
                Some(gst::PadProbeData::Buffer(ref buf)) => {
                    println!("buf {:?}", buf);
                }
                _ => (),
            }
            gst::PadProbeReturn::Ok
        });

        // Create closure so we can use ?-operator in there.
        let insert_sink = move || -> anyhow::Result<()> {
            let filesink = gst::ElementFactory::make("filesink", None)
                .map_err(|_| anyhow!("cannot create filesink"))?;
            filesink.set_property("location", OUTPUT_FILE);
            filesink.set_property("sync", &true);
            filesink.set_property("async", &false);

            let elements = &[&mp4mux, &filesink];
            pipeline.add_many(elements)?;
            gst::Element::link_many(elements)?;

            // !!ATTENTION!!:
            // This is quite important and people forget it often. Without making sure that
            // the new elements have the same state as the pipeline, things will fail later.
            // They would still be in Null state and can't process data.
            for e in elements {
                e.sync_state_with_parent()?;
            }

            // Get the mp4mux element's sink pad and link the demux's newly created
            // src pad for the video stream to it.
            let sink_pad = mp4mux
                .request_pad_simple("video_0")
                .ok_or_else(|| anyhow!("mp4mux does not have `video_0` pad"))?;
            src_pad
                .link(&sink_pad)
                .map_err(|_| anyhow!("cannot link `demux` to `mp4mux`"))?;

            Ok(())
        };

        if let Err(e) = insert_sink() {
            println!("cannot insert_sync {:?}", e);
        }
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
    std::thread::sleep(Duration::from_secs(10));

    println!("send EOS");
    ret_pipeline.send_event(gst::event::Eos::new());
    thread.join().unwrap();

    Ok(())
}
