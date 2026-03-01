const fs = require('fs')
const path = require('path')
const { Resvg } = require('@resvg/resvg-js')
const { parseICO } = require('icojs')
const pngToIco = require('png-to-ico')

const repoRoot = path.resolve(__dirname, '..', '..')
const sourceIco = process.env.WUNDER_ICON_ICO
  ? path.resolve(process.env.WUNDER_ICON_ICO)
  : path.join(repoRoot, 'images', 'eva01-head.ico')
const sourceSvg = process.env.WUNDER_ICON_SVG
  ? path.resolve(process.env.WUNDER_ICON_SVG)
  : path.join(repoRoot, 'images', 'eva01-head.svg')
const minPngSize = 512

const pngTargets = [path.join(repoRoot, 'wunder-desktop-electron', 'build', 'icon.png')]
const icoTargets = [
  path.join(repoRoot, 'wunder-desktop-electron', 'build', 'icon.ico'),
  path.join(repoRoot, 'wunder-desktop-electron', 'assets', 'icon.ico'),
  path.join(repoRoot, 'wunder-desktop', 'icons', 'icon.ico')
]

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
  let pngBuffer = Buffer.from(largest.buffer)
  let pngSource = sourceIco
  if ((largest.width < minPngSize || largest.height < minPngSize) && fs.existsSync(sourceSvg)) {
    const svgBuffer = fs.readFileSync(sourceSvg)
    pngBuffer = renderPng(svgBuffer, 1024)
    pngSource = sourceSvg
    console.log(
      `[sync-icons] ${sourceIco} max size is ${largest.width}x${largest.height}, use ${sourceSvg} for icon.png`
    )
  }

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
    `[sync-icons] synced png from ${pngSource} and ico from ${sourceIco} -> ${pngTargets.length} png target(s), ${icoTargets.length} ico target(s)`
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
  const icoPngBuffers = icoSizes.map((size) => renderPng(svgBuffer, size))
  const icoBuffer = await pngToIco(icoPngBuffers)

  for (const target of icoTargets) {
    ensureDir(target)
    fs.writeFileSync(target, icoBuffer)
  }

  console.log(
    `[sync-icons] synced from ${sourceSvg} -> ${pngTargets.length} png target(s), ${icoTargets.length} ico target(s)`
  )
  return true
}

async function main() {
  if (await writeFromIco()) {
    return
  }
  if (await writeFromSvg()) {
    return
  }
  throw new Error(`[sync-icons] icon source not found: ${sourceIco} or ${sourceSvg}`)
}

main().catch((error) => {
  console.error(String(error?.stack || error))
  process.exitCode = 1
})
