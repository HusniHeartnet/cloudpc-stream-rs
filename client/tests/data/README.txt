Klip uji sintetis (pola "testsrc2" FFmpeg, 640x360, 30 FPS, 1 detik), dibuat dengan:
  ffmpeg -f lavfi -i testsrc2=size=640x360:rate=30 -t 1 -pix_fmt yuv420p -c:v libx264 -bf 0 -g 30 testsrc.h264
  ... -c:v libx265 -x265-params bframes=0 testsrc.hevc
  ... -c:v libsvtav1 -svtav1-params pred-struct=1 -f obu testsrc.obu
Dipakai client/tests/decode_files.rs dan CI macOS (decode-test).
