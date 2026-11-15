# Wunder Linux 安装器开场画面：先把命令行版的心舰像素 logo 从块底升起来。
#
# 像素数据、灰度梯与尺寸由 build_linux_installer.py 从
# crates/wunder-cli/welcome_logo_art.rs 注入，命令行版换 logo 后安装包自动
# 跟随，不再维护第二份像素；注入前占位符原样保留，此时只打印文字标题。
#
# 输出走 stdout（摘要也走 stdout，进度条走 stderr），所以 stdout 被重定向时
# 退化为一次性静态画面，日志里依然能看到品牌行。

WUNDER_LOGO_ROWS='__WUNDER_LOGO_ROWS__'
WUNDER_LOGO_GREYS='__WUNDER_LOGO_GREYS__'
WUNDER_LOGO_COLUMNS='__WUNDER_LOGO_COLUMNS__'
WUNDER_LOGO_LINES='__WUNDER_LOGO_LINES__'
WUNDER_BANNER_VERSION='__WUNDER_VERSION__'
WUNDER_BANNER_ARCH='__WUNDER_ARCH__'

wunder_banner_title() {
  info=''
  case "$WUNDER_BANNER_VERSION" in
    ''|__*) ;;
    *) info=$WUNDER_BANNER_VERSION ;;
  esac
  case "$WUNDER_BANNER_ARCH" in
    ''|__*) ;;
    *) info="${info:+$info · }linux-$WUNDER_BANNER_ARCH" ;;
  esac
  if [ -n "${NO_COLOR:-}" ]; then
    printf 'wunder 心舰%s\n' "${info:+  $info}"
  else
    printf '\033[1mwunder 心舰\033[0m%s\n' "${info:+  $info}"
  fi
}

# 只用中性灰：与命令行欢迎画面的灰度阶梯同源，没有彩色。
# 真彩终端直接写 38;2，和命令行版输出的灰阶完全一样；只有 256 色时退到
# 232-255 灰阶（最亮约 238，比真彩略暗一档）；都没有就直接用块字符表达明暗。
wunder_banner_palette() {
  if [ -n "${NO_COLOR:-}" ]; then
    printf 'plain'
    return 0
  fi
  case "${TERM:-}" in
    ''|dumb|unknown|cons25|emacs)
      printf 'plain'
      return 0
      ;;
  esac
  case "${COLORTERM:-}" in
    truecolor|24bit)
      printf 'rgb'
      return 0
      ;;
  esac
  case "${TERM:-}" in
    *256color*)
      printf '256'
      return 0
      ;;
  esac
  printf 'plain'
}

# half-block 需要 UTF-8 终端；否则退回 ASCII 亮度斜坡。
wunder_banner_unicode() {
  locale_name=${LC_ALL:-${LC_CTYPE:-${LANG:-}}}
  case "$locale_name" in
    *[Uu][Tt][Ff]-8*|*[Uu][Tt][Ff]8*) printf 1 ;;
    *) printf 0 ;;
  esac
}

# 终端尺寸取自 stderr 上的 tty：在 $( ) 里 stdout 已被换成捕获管道，
# 只有 stderr 仍是终端。没有终端时返回 0，由调用方兜底。
wunder_terminal_size() {
  size=''
  if [ -t 2 ]; then
    # 重定向顺序不能反：先复制 tty 到 stdin，再把 stderr 静音。
    size=$(stty size <&2 2>/dev/null) || size=''
  fi
  case "$size" in
    [0-9]*' '[0-9]*)
      case "$1" in
        rows) printf '%s' "${size%% *}" ;;
        *) printf '%s' "${size##* }" ;;
      esac
      return 0
      ;;
  esac
  printf '0'
}

wunder_banner_lines() {
  printf '%s' "$(wunder_terminal_size rows)"
}

# 列数以终端实测为准，COLUMNS 只是没有终端时的兜底。
wunder_banner_columns() {
  measured=$(wunder_terminal_size cols)
  if [ "$measured" -ge 20 ]; then
    printf '%s' "$measured"
    return 0
  fi
  columns=${COLUMNS:-0}
  case "$columns" in
    ''|*[!0-9]*) columns=0 ;;
  esac
  if [ "$columns" -lt 20 ]; then
    columns=80
  fi
  printf '%s' "$columns"
}

# 单元格的高宽比：一个字符格有几倍宽那么高。半块像素只有在 2.0 时才是方的，
# 行高更大的终端（HiDPI、VS Code 集成终端、调过行距的 VTE）按 2.0 画出来的
# 头像会被纵向抻长，远看就是"被横向挤扁"。
#
# 取法与命令行版一致：显式覆盖 → 问终端（CSI 14t 文本区像素尺寸 + CSI 18t
# 行列数）→ 兜底 2.1。VTE 系终端对 TIOCGWINSZ 的像素字段回 0，所以这里直
# 接问终端；问不到就退回兜底值，绝不猜。
#
# 结果写进 WUNDER_CELL_ASPECT 而不是 stdout：查询要写终端，而本函数若被
# `$( )` 调用，stdout 就成了捕获管道，探测也就无从谈起。问不到时留空。
wunder_banner_cell_aspect() {
  WUNDER_CELL_ASPECT=''
  for raw in "${WUNDER_BANNER_CELL_ASPECT:-}" "${WUNDER_CLI_CELL_ASPECT:-}"; do
    [ -n "$raw" ] || continue
    plausible=$(awk -v value="$raw" 'BEGIN {
      if (value !~ /^[0-9]+(\.[0-9]+)?$/) exit 1
      if (value + 0 < 1.2 || value + 0 > 4.0) exit 1
      print value
    }') || plausible=''
    if [ -n "$plausible" ]; then
      WUNDER_CELL_ASPECT=$plausible
      return 0
    fi
  done

  # 只有真终端才会应答；重定向或管道里发查询只会把序列写进别人的文件。
  [ -t 0 ] && [ -t 1 ] || return 0
  case "${TERM:-}" in
    ''|dumb|unknown) return 0 ;;
  esac
  command -v stty >/dev/null 2>&1 || return 0
  command -v dd >/dev/null 2>&1 || return 0
  saved=$(stty -g 2>/dev/null) || saved=''
  [ -n "$saved" ] || return 0
  # 不关 echo 的话，终端的应答会被回显成乱码；min 0 time 1 让读取 0.1s 就返回，
  # 完全不响应的终端最多拖慢三拍。
  if ! stty -echo -icanon min 0 time 1 2>/dev/null; then
    stty "$saved" 2>/dev/null || :
    return 0
  fi
  if ! printf '\033[14t\033[18t'; then
    stty "$saved" 2>/dev/null || :
    return 0
  fi
  replies=''
  for _ in 1 2 3; do
    chunk=$(dd bs=32 count=1 2>/dev/null) || chunk=''
    [ -n "$chunk" ] || break
    replies=$replies$chunk
  done
  stty "$saved" 2>/dev/null || :
  aspect=$(printf '%s' "$replies" | awk '
  BEGIN { esc = sprintf("%c", 27) }
  {
    text = $0
    while ((start = index(text, esc "[")) > 0) {
      text = substr(text, start + 2)
      stop = index(text, "t")
      if (stop == 0) break
      params = substr(text, 1, stop - 1)
      if (split(params, part, ";") == 3) {
        if (part[1] == 4) pixels = part[2] " " part[3]
        else if (part[1] == 8) cells = part[2] " " part[3]
      }
      text = substr(text, stop + 1)
    }
  }
  END {
    if (pixels == "" || cells == "") exit 1
    split(pixels, area, " ")
    split(cells, grid, " ")
    if (area[1] <= 0 || area[2] <= 0 || grid[1] <= 0 || grid[2] <= 0) exit 1
    aspect = (area[1] / grid[1]) / (area[2] / grid[2])
    if (aspect < 1.2 || aspect > 4.0) exit 1
    printf "%.4f", aspect
  }') || aspect=''
  [ -n "$aspect" ] || return 0
  WUNDER_CELL_ASPECT=$aspect
  return 0
}

# 画多少列、多少行：与命令行版 fit() 同一公式。
# 一格装两个源像素行，源图 40×34 保持方正需要
#   rows = width · 34 / (40 · cell_aspect)
# 行数上限是 17（半块一格能表达的分辨率上限），宽度上限 40（源图宽度）。
# 输出 "<列数> <行数>"，终端太窄时输出 "0 0"。
wunder_banner_geometry() {
  awk -v columns="$1" -v aspect="$2" 'BEGIN {
    width = columns - 2
    if (width > 40) width = 40
    if (width < 0) width = 0
    ratio = aspect + 0
    if (ratio < 1.0) ratio = 1.0
    rows = int(width * 34 / (40 * ratio) + 0.5)
    if (rows > 17) rows = 17
    if (rows < 1) rows = 1
    if (width < 24 || rows < 6) { printf "0 0"; exit 0 }
    printf "%d %d", width, rows
  }'
}

# 画一帧：`visible` 是已经升进画面的行数，1 行时只露头尖。
#
# 采样与命令行版 render() 完全一致：第 row 行取源图的
#   [row·34/rows, (row+1)·34/rows) 这一段的首尾两个像素行，
# 列按 width 等比映射。上下半块各拿一个像素，不做跨行平均——平均会把五官
# 糊成横条，也是早先"发扁"的来由。
wunder_banner_frame() {
  awk -v visible="$1" -v width="$2" -v rows="$3" -v mode="$4" \
    -v art_rows="$WUNDER_LOGO_ROWS" -v art_greys="$WUNDER_LOGO_GREYS" \
    -v art_columns="$WUNDER_LOGO_COLUMNS" -v art_lines="$WUNDER_LOGO_LINES" '
  # 灰度值只走中性灰：真彩写 38;2;v;v;v，256 色终端走 232-255 灰阶。
  # 同色相邻格不重复发 SGR，慢终端（ssh、串口）也能流畅刷帧。
  function paint_fg(value) {
    if (value == last_fg) return ""
    last_fg = value
    if (mode == "rgb") return sprintf("%s[38;2;%d;%d;%dm", ESC, value, value, value)
    return sprintf("%s[38;5;%dm", ESC, 232 + int(value * 23 / 255 + 0.5))
  }
  function paint_bg(value) {
    if (value == last_bg) return ""
    last_bg = value
    if (mode == "rgb") return sprintf("%s[48;2;%d;%d;%dm", ESC, value, value, value)
    return sprintf("%s[48;5;%dm", ESC, 232 + int(value * 23 / 255 + 0.5))
  }
  # 源像素亮度；-1 表示透明。一行 hex 里每两个字符装上下两个像素。
  function shade(x, y,   row, pair, hi, lo, slot) {
    row = source[int(y / 2) + 1]
    pair = substr(row, int(x) * 2 + 1, 2)
    hi = index(HEX, substr(pair, 1, 1)) - 1
    lo = index(HEX, substr(pair, 2, 1)) - 1
    slot = (y % 2 == 0) ? hi : lo
    if (slot < 0 || slot >= levels) return -1
    return ladder[slot + 1] + 0
  }
  # 透明格必须回到默认背景：终端里空格也带着当前背景色，残留的背景会
  # 在轮廓右侧铺出一块灰底。
  function cell(top, bottom,   value, glyph) {
    if (top < 0 && bottom < 0) {
      if (ansi_now && (last_fg >= 0 || last_bg >= 0)) {
        last_fg = -1
        last_bg = -1
        return ESC "[0m "
      }
      return " "
    }
    if (mode == "ascii") {
      if (top < 0) value = bottom
      else if (bottom < 0) value = top
      else value = int((top + bottom) / 2)
      glyph = int(value * 9 / 255 + 0.5)
      return substr(RAMP, glyph + 1, 1)
    }
    if (mode == "plain") {
      if (top < 0) value = bottom
      else if (bottom < 0) value = top
      else value = int((top + bottom) / 2)
      if (value >= 190) return "█"
      if (value >= 115) return "▀"
      if (value >= 45) return "▄"
      return " "
    }
    # 只有一侧有像素时，另一侧露出来的就是背景色：背景必须复位到默认，
    # 否则上一格留下的底色会顺着轮廓铺出一圈灰边（命令行版每格都自带默认背景）。
    if (top < 0) {
      reset = ""
      if (last_bg >= 0) {
        last_bg = -1
        reset = ESC "[49m"
      }
      return paint_fg(bottom) reset "▄"
    }
    if (bottom < 0) {
      reset = ""
      if (last_bg >= 0) {
        last_bg = -1
        reset = ESC "[49m"
      }
      return paint_fg(top) reset "▀"
    }
    if (top == bottom) return paint_fg(top) "█"
    return paint_fg(top) paint_bg(bottom) "▀"
  }
  BEGIN {
    ESC = sprintf("%c", 27)
    HEX = "0123456789abcdef"
    RAMP = " .:-=+*#%@"
    ansi_now = (mode == "ascii" || mode == "plain") ? 0 : 1
    levels = split(art_greys, ladder, " ")
    source_count = split(art_rows, source, " ")
    # 数据与声明对不上就别画：宁可只留标题，也不画一张错位的图。
    if (source_count < 2 || source_count != art_lines + 0 || levels < 2) exit 0
    if (art_columns + 0 < 4 || width + 0 < 1 || rows + 0 < 1) exit 0
    source_width = art_columns + 0
    source_height = source_count * 2
    if (visible < 0) visible = 0
    if (visible > rows) visible = rows
    blank = rows - visible
    for (row = 0; row < rows; row++) {
      # 还没升上来的行留空：块高固定，下面的进度条和摘要不会跟着跳。
      if (row < blank) {
        if (ansi_now) printf "%s%s\n", ESC "[0m", ESC "[K"
        else printf "\n"
        continue
      }
      art = row - blank
      y_top = int(art * source_height / rows)
      y_bottom = int((art + 1) * source_height / rows) - 1
      if (y_bottom < y_top) y_bottom = y_top
      line = ""
      last_fg = -1
      last_bg = -1
      for (column = 0; column < width; column++) {
        x = int(column * source_width / width)
        line = line cell(shade(x, y_top), shade(x, y_bottom))
      }
      if (ansi_now) printf "%s%s%s\n", line, ESC "[0m", ESC "[K"
      else printf "%s\n", line
    }
  }'
}

wunder_banner() {
  [ "${WUNDER_BANNER:-1}" = "1" ] || return 0

  case "$WUNDER_LOGO_ROWS" in
    ''|__*|*[!0-9a-f\ ]*)
      printf '\n'
      wunder_banner_title
      printf '\n'
      return 0
      ;;
  esac
  if ! command -v awk >/dev/null 2>&1; then
    printf '\n'
    wunder_banner_title
    printf '\n'
    return 0
  fi

  mode=$(wunder_banner_palette)
  # 输出被重定向（发到日志或管道）时不写 SGR 序列，日志保持可读。
  if [ ! -t 1 ]; then
    mode=plain
  fi
  if [ "$(wunder_banner_unicode)" -eq 0 ]; then
    mode=ascii
  fi

  columns=$(wunder_banner_columns)
  wunder_banner_cell_aspect
  aspect=${WUNDER_CELL_ASPECT:-2.1}
  geometry=$(wunder_banner_geometry "$columns" "$aspect")
  width=${geometry%% *}
  out_rows=${geometry##* }
  case "$width$out_rows" in
    ''|*[!0-9]*)
      width=0
      out_rows=0
      ;;
  esac
  if [ "$width" -lt 1 ] || [ "$out_rows" -lt 1 ]; then
    printf '\n'
    wunder_banner_title
    printf '\n'
    return 0
  fi

  # 终端比图还矮时不播放动画：宁可静态一帧，也不把标题顶出可视区。
  height=$(wunder_banner_lines)
  live=0
  if [ -t 1 ] && [ "${WUNDER_PROGRESS:-1}" -eq 1 ]; then
    live=1
    if [ "$height" -gt 0 ] && [ "$height" -lt $((out_rows + 5)) ]; then
      live=0
    fi
  fi

  printf '\n'
  if [ "$live" -eq 1 ]; then
    # 一行一行升上来：整块 0.65s 左右升到位，每帧先回到块顶再重画。
    delay=$(awk -v rows="$out_rows" 'BEGIN { printf "%.3f", 0.65 / rows }')
    visible=1
    while [ "$visible" -le "$out_rows" ]; do
      # 画图失败也不能挡住安装：开场画面只是仪式感。
      wunder_banner_frame "$visible" "$width" "$out_rows" "$mode" || true
      if [ "$visible" -lt "$out_rows" ]; then
        printf '\033[%dA' "$out_rows"
        sleep "$delay" 2>/dev/null || :
      fi
      visible=$((visible + 1))
    done
  else
    wunder_banner_frame "$out_rows" "$width" "$out_rows" "$mode" || true
  fi
  printf '\n'
  wunder_banner_title
  printf '\n'
  return 0
}
