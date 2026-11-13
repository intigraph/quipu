"""Replay the recorded output; --speed changes only the playback delay."""
import argparse
import json
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument("recording")
parser.add_argument("--speed", type=float, default=1.0)
args = parser.parse_args()
if args.speed <= 0:
    parser.error("speed must be positive")
with open(args.recording, encoding="utf-8") as recording:
    header = json.loads(next(recording))
    assert header["version"] == 2
    previous = 0.0
    for line in recording:
        timestamp, kind, output = json.loads(line)
        assert timestamp >= previous and kind == "o"
        time.sleep((timestamp - previous) / args.speed)
        sys.stdout.buffer.write(output.encode())
        sys.stdout.buffer.flush()
        previous = timestamp
