const fs = require('fs')
const path = require('path')
const { Resvg } = require('@resvg/resvg-js')
const { parseICO } = require('icojs')
const pngToIcoModule = require('png-to-ico')
const pngToIco = pngToIcoModule.default ?? pngToIcoModule
const { PNG } = require('pngjs')

const repoRoot = path.resolve(__dirname, '..', '..', '..')
const sourcePng = process.env.WUNDER_ICON_PNG
  ? path.resolve(process.env.WUNDER_ICON_PNG)
  : path.join(repoRoot, 'images', 'eva01-head.png')
const sourceSvg = process.env.WUNDER_ICON_SVG
  ? path.resolve(process.env.WUNDER_ICON_SVG)
  : ''
const sourceIco = process.env.WUNDER_ICON_ICO
  ? path.resolve(process.env.WUNDER_ICON_ICO)
  : ''
const defaultIcoPaddingRatio = 0.06

const pngTargets = [path.join(repoRoot, 'desktop', 'electron', 'build', 'icon.png')]
const icoTargets = [
  path.join(repoRoot, 'desktop', 'electron', 'build', 'icon.ico'),
  path.join(repoRoot, 'desktop', 'electron', 'assets', 'icon.ico'),
  path.join(repoRoot, 'desktop', 'tauri', 'icons', 'icon.ico')
]

const normalizePaddingRatio = (rawValue) => {
  const parsed = Number(rawValue)
  if (!Number.isFinite(parsed)) {
    return defaultIcoPaddingRatio
  }
  return Math.min(Math.max(parsed, 0), 0.3)
}

const icoPaddingRatio = normalizePaddingRatio(process.env.WUNDER_ICON_PADDING_RATIO)

const ensureDir = (targetPath) => {
  fs.mkdirSync(path.dirname(targetPath), { recursive: true })
}

const renderPng = (svgBuffer, size) =>
  new Resvg(svgBuffer, {
    fitTo: {
      mode: 'width',
      value: size
    }
  })
    .render()
    .asPng()

const containPngInSquare = (sourcePng, size) => {
  const scale = Math.min(size / sourcePng.width, size / sourcePng.height)
  const width = Math.max(1, Math.round(sourcePng.width * scale))
  const height = Math.max(1, Math.round(sourcePng.height * scale))
  const resized = new PNG({ width: size, height: size })
  const offsetX = Math.floor((size - width) / 2)
  const offsetY = Math.floor((size - height) / 2)
  for (let y = 0; y < height; y += 1) {
    const sourceY = Math.min(sourcePng.height - 1, Math.floor(y / scale))
    for (let x = 0; x < width; x += 1) {
      const sourceX = Math.min(sourcePng.width - 1, Math.floor(x / scale))
      const sourceIndex = (sourceY * sourcePng.width + sourceX) * 4
      const targetIndex = ((offsetY + y) * size + offsetX + x) * 4
      resized.data[targetIndex] = sourcePng.data[sourceIndex]
      resized.data[targetIndex + 1] = sourcePng.data[sourceIndex + 1]
      resized.data[targetIndex + 2] = sourcePng.data[sourceIndex + 2]
      resized.data[targetIndex + 3] = sourcePng.data[sourceIndex + 3]
    }
  }
  return PNG.sync.write(resized)
}

const resizePng = (pngBuffer, size) => {
  const source = PNG.sync.read(pngBuffer)
  if (source.width === size && source.height === size) {
    return pngBuffer
  }
  return containPngInSquare(source, size)
}

const stretchPngToSquare = (pngBuffer, size) => {
  const source = PNG.sync.read(pngBuffer)
  if (source.width === size && source.height === size) {
    return pngBuffer
  }
  const resized = new PNG({ width: size, height: size })
  for (let y = 0; y < size; y += 1) {
    const sourceY = Math.min(source.height - 1, Math.floor((y * source.height) / size))
    for (let x = 0; x < size; x += 1) {
      const sourceX = Math.min(source.width - 1, Math.floor((x * source.width) / size))
      const sourceIndex = (sourceY * source.width + sourceX) * 4
      const targetIndex = (y * size + x) * 4
      resized.data[targetIndex] = source.data[sourceIndex]
      resized.data[targetIndex + 1] = source.data[sourceIndex + 1]
      resized.data[targetIndex + 2] = source.data[sourceIndex + 2]
      resized.data[targetIndex + 3] = source.data[sourceIndex + 3]
    }
  }
  return PNG.sync.write(resized)
}

const addTransparentPadding = (pngBuffer, size, resize = resizePng) => {
  const basePng = resize(pngBuffer, size)
  if (size <= 2 || icoPaddingRatio <= 0) {
    return basePng
  }
  const padding = Math.max(1, Math.round(size * icoPaddingRatio))
  const innerSize = Math.max(1, size - padding * 2)
  if (innerSize >= size) {
    return basePng
  }
  const innerPng = PNG.sync.read(resize(pngBuffer, innerSize))
  const canvas = new PNG({ width: size, height: size })
  PNG.bitblt(innerPng, canvas, 0, 0, innerPng.width, innerPng.height, padding, padding)
  return PNG.sync.write(canvas)
}

const uniquePaths = (paths) => [...new Set(paths.map((item) => path.resolve(item)))]

const writeFromPng = async () => {
  if (!fs.existsSync(sourcePng)) {
    return false
  }

  const pngBuffer = fs.readFileSync(sourcePng)
  const appPngBuffer = resizePng(pngBuffer, 512)
  for (const target of pngTargets) {
    ensureDir(target)
    fs.writeFileSync(target, appPngBuffer)
  }

  const icoSizes = [16, 24, 32, 48, 64, 128, 256]
  // Windows shell icons are rendered from square resource frames; stretching the
  // source keeps shortcut and taskbar icons visually consistent with the app icon.
  const icoPngBuffers = icoSizes.map((size) => addTransparentPadding(pngBuffer, size, stretchPngToSquare))
  const icoBuffer = await pngToIco(icoPngBuffers)

  for (const target of icoTargets) {
    ensureDir(target)
    fs.writeFileSync(target, icoBuffer)
  }

  console.log(
    `[sync-icons] generated ico from ${sourcePng} -> ${pngTargets.length} png target(s), ${icoTargets.length} ico target(s)`
  )
  return true
}

const writeFromSvg = async () => {
  if (!fs.existsSync(sourceSvg)) {
    return false
  }

  const svgBuffer = fs.readFileSync(sourceSvg)
  const png1024 = renderPng(svgBuffer, 1024)

  for (const target of pngTargets) {
    ensureDir(target)
    fs.writeFileSync(target, png1024)
  }

  const icoSizes = [16, 24, 32, 48, 64, 128, 256]
  const icoPngBuffers = icoSizes.map((size) => addTransparentPadding(png1024, size))
  const icoBuffer = await pngToIco(icoPngBuffers)

  const allIcoTargets = uniquePaths([sourceIco, ...icoTargets].filter(Boolean))
  for (const target of allIcoTargets) {
    ensureDir(target)
    fs.writeFileSync(target, icoBuffer)
  }

  console.log(
    `[sync-icons] generated ico from ${sourceSvg} (padding=${icoPaddingRatio}) -> ${pngTargets.length} png target(s), ${allIcoTargets.length} ico target(s)`
  )
  return true
}

const writeFromIco = async () => {
  if (!fs.existsSync(sourceIco)) {
    return false
  }
  const parsed = await parseICO(fs.readFileSync(sourceIco), 'image/png')
  if (!Array.isArray(parsed) || parsed.length === 0) {
    throw new Error(`[sync-icons] invalid ico source: ${sourceIco}`)
  }

  const largest = parsed
    .slice()
    .sort((left, right) => right.width * right.height - left.width * left.height)[0]
  const pngBuffer = Buffer.from(largest.buffer)

  for (const target of pngTargets) {
    ensureDir(target)
    fs.writeFileSync(target, pngBuffer)
  }

  const icoBuffer = fs.readFileSync(sourceIco)
  for (const target of icoTargets) {
    ensureDir(target)
    fs.writeFileSync(target, icoBuffer)
  }

  console.log(
    `[sync-icons] synced png from ${sourceIco} and ico from ${sourceIco} -> ${pngTargets.length} png target(s), ${icoTargets.length} ico target(s)`
  )
  return true
}

async function main() {
  if (await writeFromPng()) {
    return
  }
  if (await writeFromSvg()) {
    return
  }
  if (await writeFromIco()) {
    return
  }
  throw new Error(`[sync-icons] icon source not found: ${sourcePng || sourceIco || sourceSvg}`)
}

main().catch((error) => {
  console.error(String(error?.stack || error))
  process.exitCode = 1
})
