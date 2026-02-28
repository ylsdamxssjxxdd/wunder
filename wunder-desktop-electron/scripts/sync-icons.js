const fs = require('fs')
const path = require('path')
const { Resvg } = require('@resvg/resvg-js')
const pngToIco = require('png-to-ico')

const repoRoot = path.resolve(__dirname, '..', '..')
const sourceSvg = process.env.WUNDER_ICON_SVG
  ? path.resolve(process.env.WUNDER_ICON_SVG)
  : path.join(repoRoot, 'images', 'eva01-head.svg')

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

async function main() {
  if (!fs.existsSync(sourceSvg)) {
    throw new Error(`[sync-icons] source svg not found: ${sourceSvg}`)
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
}

main().catch((error) => {
  console.error(String(error?.stack || error))
  process.exitCode = 1
})
