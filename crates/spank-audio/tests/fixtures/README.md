# Audio fixtures

0.3 s silence + 1.0 s 440 Hz sine at -12 dBFS, 22.05 kHz stereo, in every supported format,
plus a 0.5 s 6-channel 16 kHz tone. Regenerate with ffmpeg (run from this directory):

```bash
ffmpeg -y -f lavfi -i anullsrc=r=22050:cl=stereo:d=0.3 -f lavfi -i sine=frequency=440:sample_rate=22050:duration=1.0 -filter_complex "[1:a]volume=0.25,aformat=channel_layouts=stereo[s];[0:a][s]concat=n=2:v=0:a=1" tone.wav
ffmpeg -y -i tone.wav -b:a 48k tone.mp3
ffmpeg -y -i tone.wav -c:a aac -b:a 48k tone.m4a
ffmpeg -y -i tone.wav -c:a aac -b:a 48k -f adts tone.aac
ffmpeg -y -i tone.wav tone.flac
ffmpeg -y -i tone.wav -c:a vorbis -strict -2 -q:a 2 tone.ogg
ffmpeg -y -f lavfi -i "sine=frequency=300:sample_rate=16000:duration=0.5" -ac 6 surround.wav
```
