import sys
import signal
import time
from xml.dom.minidom import Element

import gi
gi.require_version('Gst', '1.0')
from gi.repository import GLib, Gst, GObject

import termios, atexit, sys
from select import select

# save the terminal settings
fd = sys.stdin.fileno()
new_term = termios.tcgetattr(fd)
old_term = termios.tcgetattr(fd)

# new terminal setting unbuffered
new_term[3] = (new_term[3] & ~termios.ICANON & ~termios.ECHO)

# switch to normal terminal
def set_normal_term():
    termios.tcsetattr(fd, termios.TCSAFLUSH, old_term)

# switch to unbuffered terminal
def set_curses_term():
    termios.tcsetattr(fd, termios.TCSAFLUSH, new_term)

def putch(ch):
    sys.stdout.write(ch)

def getch():
    return sys.stdin.read(1)

def getche():
    ch = getch()
    putch(ch)
    return ch

def kbhit():
    dr,dw,de = select([sys.stdin], [], [], 0)
    return dr != []

atexit.register(set_normal_term)
set_curses_term()

def kbfunc():
    if kbhit():
        return ord(getch())

class bcolors:
    HEADER = '\033[95m'
    OKBLUE = '\033[94m'
    OKGREEN = '\033[92m'
    WARNING = '\033[93m'
    FAIL = '\033[91m'
    ENDC = '\033[0m'
    BOLD = '\033[1m'
    UNDERLINE = '\033[4m'


class Pipeline:
    def __init__(self, uri):
        self.pipeline = Gst.Pipeline()

        self.filesrc = Gst.ElementFactory.make("filesrc", "filesrc")
        self.filesrc.set_property("location", uri)
        self.pipeline.add(self.filesrc)

        self.qtdemux = Gst.ElementFactory.make("qtdemux", "qtdemux")
        self.qtdemux.connect("pad-added", self.on_demux_pad_added)
        self.pipeline.add(self.qtdemux)

        self.filesrc.link(self.qtdemux)

        self.bus = self.pipeline.get_bus()
        self.bus.add_signal_watch()
        self.bus.enable_sync_message_emission()
        self.bus.connect("message", self.on_message)
        #self.bus.connect('message::eos', self.on_eos)
        #self.bus.connect('message::error', self.on_error)

    def start(self):
        self.pipeline.set_state(Gst.State.PLAYING)

    def stop(self):
        self.pipeline.send_event(Gst.Event.new_eos())

    def on_demux_pad_added(self, demux, pad, *user_data):
        print("demux pad added")
        # Create the rest of the pipeline here and link it
        parse = Gst.ElementFactory.make("h265parse", "parse")
        mux = Gst.ElementFactory.make("mp4mux", "mp4mux")
        srcpad = mux.get_static_pad('src')
        srcpad.add_probe(Gst.PadProbeType.DATA_DOWNSTREAM, self.probe_cb, None)

        filesink = Gst.ElementFactory.make("filesink", "filesink")
        filesink.set_property("location", "/Users/andres/Downloads/out.mp4")
        filesink.set_property("sync", True)
        filesink.set_property("async", False)

        elements = [parse, mux, filesink]
        for e in elements:
            self.pipeline.add(e)

        for e in elements:
            e.sync_state_with_parent()
            e.sync_state_with_parent()

        sink_pad = parse.get_static_pad("sink")
        pad.link(sink_pad)
        #demux.link(parse)

        parse.link(mux)
        mux.link(filesink)
        return Gst.PadProbeReturn.OK

    def probe_cb(self, pad, info, pdata):
        print("probe_cb type %s" % info.type)
        if info.type & Gst.PadProbeType.BUFFER:
            b = info.get_buffer()
            print("probe_cb offset %d offset_end %d dts %s duration %s pts %s" % (b.offset, b.offset_end, b.dts, b.duration, b.pts))

        return Gst.PadProbeReturn.OK

    def quit(self):
        self.filesrc.set_state(Gst.State.NULL)
        self.loop.quit()

    # def on_eos(self, bus, msg):
    #     print(f"{bcolors.WARNING}SEEK{bcolors.ENDC}")
    #     self.filesrc.seek_simple(Gst.Format.TIME, Gst.SeekFlags.FLUSH, 0)

    def on_error(self, bus, msg):
        (err, debug) = msg.parse_error()
        print("Error: %s" % err, debug)
        self.quit()

    def on_message(self, bus, message):
        t = message.type
        print(f"{bcolors.WARNING}on_message: {t}{bcolors.ENDC}")

        if t == Gst.MessageType.EOS:
            print(f"{bcolors.FAIL}EOS{bcolors.ENDC}")
            self.filesrc.seek_simple(Gst.Format.TIME, Gst.SeekFlags.FLUSH, 0)
            #self.pipeline.set_state(Gst.State.NULL)
        elif t == Gst.MessageType.ERROR:
            self.pipeline.set_state(Gst.State.NULL)
            err, debug = message.parse_error()
            print(f"{bcolors.FAIL}ERROR {err} {debug}{bcolors.ENDC}")


Gst.init(None)

def handler(signum, frame):
    print(f"{bcolors.WARNING}CTRL+C{bcolors.ENDC}")
    pipe.stop()
    time.sleep(2.0)
    exit(0)

signal.signal(signal.SIGINT, handler)

print(f"{bcolors.BOLD}start[s], end[e], quit[q]{bcolors.ENDC}")

while 1:
    c = kbfunc()
    if c:
        c = chr(c)
        if c == 's':
            print(f"{bcolors.WARNING}START...{bcolors.ENDC}")

            pipe = Pipeline(sys.argv[1])
            pipe.start()

            import _thread
            loop = GObject.MainLoop()
            _thread.start_new_thread(loop.run, ())
        elif c == 'e':
            print(f"{bcolors.WARNING}STOP...{bcolors.ENDC}")
            pipe.stop()

        elif c == 'q':
            print(f"{bcolors.WARNING}QUIT{bcolors.ENDC}")
            exit(0)
        else:
            pass

    time.sleep(0.01)
