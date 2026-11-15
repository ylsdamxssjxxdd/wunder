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
# 有 256 色就用灰阶索引（232-255），否则直接用块字符表达明暗。
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

wunder_banner_columns() {
  columns=${COLUMNS:-0}
  case "$columns" in
    ''|*[!0-9]*) columns=0 ;;
  esac
  if [ "$columns" -lt 20 ]; then
    measured=$(wunder_terminal_size cols)
    if [ "$measured" -gt 0 ]; then
      columns=$measured
    fi
  fi
  if [ "$columns" -lt 20 ]; then
    columns=80
  fi
  printf '%s' "$columns"
}

# 画一帧：`visible` 是已经升进画面的行数，1 行时只露头尖。
#
# 和命令行版同一套观感：整块高度固定，新行出现在最下面、已有的行被顶上去，
# 头尖先露头再逐渐看全；没有逐行点亮，也没有整图闪白。
wunder_banner_frame() {
  awk -v visible="$1" -v step="$2" -v mode="$3" \
    -v rows="$WUNDER_LOGO_ROWS" -v greys="$WUNDER_LOGO_GREYS" '
  # 灰度值只走 256 色的 232-255 灰阶：白灰层次，不带任何彩色。
  # 同色相邻格不重复发 SGR，慢终端（ssh、串口）也能流畅刷帧。
  function paint_fg(value) {
    if (value == last_fg) return ""
    last_fg = value
    return sprintf("%s[38;5;%dm", ESC, 232 + int(value * 23 / 255 + 0.5))
  }
  function paint_bg(value) {
    if (value == last_bg) return ""
    last_bg = value
    return sprintf("%s[48;5;%dm", ESC, 232 + int(value * 23 / 255 + 0.5))
  }
  # 源像素亮度；-1 表示透明。
  function shade(x, y,   row, pair, hi, lo, slot) {
    row = source[int(y / 2) + 1]
    pair = substr(row, int(x) * 2 + 1, 2)
    hi = index(HEX, substr(pair, 1, 1)) - 1
    lo = index(HEX, substr(pair, 2, 1)) - 1
    slot = (y % 2 == 0) ? hi : lo
    if (slot < 0 || slot >= levels) return -1
    return ladder[slot + 1] + 0
  }
  # 一个格子的上半或下半：step 列 × step 像素行取平均。
  #
  # 上下两半各取 step 行、起始相差 step 行，合起来正好 2*step 行。早先这里
  # 每半取 2*step 行，相邻两半重叠一半，等于把上下两行糊成一片——头像的
  # 五官全被抹平，看上去就是一个扁三角。命令行版是一像素一格，没有这道
  # 平均，所以同一个 logo 在那边是立体的。
  function block(x0, y0,   sum, count, dy, dx, value) {
    sum = 0
    count = 0
    for (dy = 0; dy < step; dy++) {
      for (dx = 0; dx < step; dx++) {
        value = shade(x0 + dx, y0 + dy)
        if (value >= 0) {
          sum += value
          count++
        }
      }
    }
    if (count == 0) return -1
    return int(sum / count)
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
    if (top < 0) return paint_fg(bottom) "▄"
    if (bottom < 0) return paint_fg(top) "▀"
    if (top == bottom) return paint_fg(top) "█"
    return paint_fg(top) paint_bg(bottom) "▀"
  }
  BEGIN {
    ESC = sprintf("%c", 27)
    HEX = "0123456789abcdef"
    RAMP = " .:-=+*#%@"
    ansi_now = (mode == "ascii" || mode == "plain") ? 0 : 1
    levels = split(greys, ladder, " ")
    source_count = split(rows, source, " ")
    width = int(length(source[1]) / 2)
    out_rows = int((source_count + step - 1) / step)
    if (width < 4 || out_rows < 2 || levels < 2) exit 0
    if (visible < 0) visible = 0
    if (visible > out_rows) visible = out_rows
    blank = out_rows - visible
    for (row = 0; row < out_rows; row++) {
      # 还没升上来的行留空：块高固定，下面的进度条和摘要不会跟着跳。
      if (row < blank) {
        if (ansi_now) printf "%s%s\n", ESC "[0m", ESC "[K"
        else printf "\n"
        continue
      }
      line = ""
      last_fg = -1
      last_bg = -1
      art = row - blank
      for (column = 0; column < width; column += step) {
        top = block(column, art * 2 * step)
        bottom = block(column, art * 2 * step + step)
        line = line cell(top, bottom)
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
  step=1
  if [ "$columns" -lt $((WUNDER_LOGO_COLUMNS + 4)) ]; then
    step=2
  fi
  out_rows=$(( (WUNDER_LOGO_LINES + step - 1) / step ))

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
      wunder_banner_frame "$visible" "$step" "$mode" || true
      if [ "$visible" -lt "$out_rows" ]; then
        printf '\033[%dA' "$out_rows"
        sleep "$delay" 2>/dev/null || :
      fi
      visible=$((visible + 1))
    done
  else
    wunder_banner_frame "$out_rows" "$step" "$mode" || true
  fi
  printf '\n'
  wunder_banner_title
  printf '\n'
  return 0
}
