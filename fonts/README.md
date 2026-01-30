# Fonts directory

Drop CJK font files here (e.g., .otf/.ttf/.ttc). docker-compose mounts this
folder to `/usr/share/fonts/wunder` inside the `wunder_engine` and `sandbox`
containers.

Recommended families:
- Noto Sans CJK (SC/TC/JP/KR)
- Source Han Sans (SC/CN)
- SimSun (simsun.ttc)
- FangSong (simfang.ttf)
- SimHei (simhei.ttf)
- KaiTi (simkai.ttf)
- Microsoft YaHei (msyh.ttc)

After adding fonts, restart the containers so Matplotlib can rebuild its cache.
