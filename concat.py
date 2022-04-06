import sys
import signal
import time

import gi
gi.require_version('Gst', '1.0')
from gi.repository import GLib, Gst, GObject

import termios, atexit, sys
from select import select

# https://gstreamer.freedesktop.org/documentation/application-development/advanced/pipeline-manipulation.html?gi-language=c

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
        self.uri = uri
        self.pipeline = Gst.Pipeline()

        # prepare 5 inputs for the `concat` element
        # if first filesrc is read then concat takes another until all srcs are read
        # this is used to loop the same file multiple times
        for i in range(5):
            filesrc = Gst.ElementFactory.make("filesrc", "filesrc%d" % i)
            filesrc.set_property("location", uri)
            self.pipeline.add(filesrc)

            qtdemux = Gst.ElementFactory.make("qtdemux", "qtdemux%d" % i)
            qtdemux.connect("pad-added", self.on_demux_pad_added, i)
            self.pipeline.add(qtdemux)

            filesrc.link(qtdemux)

        self.concat = Gst.ElementFactory.make("concat", "concat")
        parse = Gst.ElementFactory.make("h265parse", "parse")
        mux = Gst.ElementFactory.make("mp4mux", "mp4mux")
        srcpad = mux.get_static_pad('src')
        srcpad.add_probe(Gst.PadProbeType.DATA_DOWNSTREAM, self.probe_cb, None)
        filesink = Gst.ElementFactory.make("filesink", "filesink")
        filesink.set_property("location", "/Users/andres/Downloads/out.mp4")
        filesink.set_property("sync", True)
        filesink.set_property("async", False)

        self.pipeline.add(self.concat, parse, mux, filesink)
        self.concat.link(parse)
        parse.link(mux)
        mux.link(filesink)

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
        print(f"{bcolors.WARNING}qtdemux{user_data} pad added{bcolors.ENDC}")

        sink_pad = self.concat.request_pad_simple("sink_%d" % user_data)

        pad.link(sink_pad)
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

    def on_error(self, bus, msg):
        (err, debug) = msg.parse_error()
        print("Error: %s" % err, debug)
        self.quit()

    def on_message(self, bus, message):
        t = message.type
        print(f"{bcolors.WARNING}on_message: {t}{bcolors.ENDC}")

        if t == Gst.MessageType.EOS:
            print(f"{bcolors.FAIL}EOS{bcolors.ENDC}")
            self.pipeline.set_state(Gst.State.NULL)
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
