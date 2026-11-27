// Builds every Veritas brand asset from two sources: the traced wordmark
// (design/brand/source/wordmark-traced.svg, from trace-wordmark.swift) and the
// coin, drawn here from the geometry of Bootstrap Icons' "coin" (MIT).
//
//     swift tools/brand/build-brand.swift
//
// Run it from the repository root. It rewrites design/brand/ (SVGs, PNGs,
// favicon, menu bar template, the Icon Composer app icon and the previews),
// apps/macos/Veritas/AppIcon.icon, and VeritasDesign's BrandArtwork.swift.

import AppKit
import CoreGraphics

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let brand = root.appending(path: "design/brand")
let fm = FileManager.default

enum Palette {
	static let primary = "#23169C"
	static let primaryDarkMode = "#98ACFF"
	static let dark = "#141418"
	static let black = "#000000"
	static let white = "#FFFFFF"
}

// MARK: - Path text

func number(_ v: CGFloat) -> String {
	let r = (Double(v) * 100).rounded() / 100
	if r == r.rounded() { return String(Int(r)) }
	var s = String(format: "%.2f", r)
	while s.hasSuffix("0") { s.removeLast() }
	return s
}

func svgPath(_ path: CGPath) -> String {
	var d = ""
	var current = CGPoint.zero
	path.applyWithBlock { pointer in
		let e = pointer.pointee
		let p = e.points
		switch e.type {
		case .moveToPoint:
			d += "M\(number(p[0].x)) \(number(p[0].y))"; current = p[0]
		case .addLineToPoint:
			d += "L\(number(p[0].x)) \(number(p[0].y))"; current = p[0]
		case .addQuadCurveToPoint:
			let c1 = CGPoint(x: current.x + 2 / 3 * (p[0].x - current.x), y: current.y + 2 / 3 * (p[0].y - current.y))
			let c2 = CGPoint(x: p[1].x + 2 / 3 * (p[0].x - p[1].x), y: p[1].y + 2 / 3 * (p[0].y - p[1].y))
			d += "C\(number(c1.x)) \(number(c1.y)) \(number(c2.x)) \(number(c2.y)) \(number(p[1].x)) \(number(p[1].y))"; current = p[1]
		case .addCurveToPoint:
			d += "C\(number(p[0].x)) \(number(p[0].y)) \(number(p[1].x)) \(number(p[1].y)) \(number(p[2].x)) \(number(p[2].y))"; current = p[2]
		case .closeSubpath:
			d += "Z"
		@unknown default:
			break
		}
	}
	return d
}

/// Reads the absolute M/L/C/Z path data this toolchain writes.
func parsePath(_ d: String) -> CGPath {
	let path = CGMutablePath()
	let scanner = Scanner(string: d)
	scanner.charactersToBeSkipped = CharacterSet(charactersIn: " ,\n")
	func point() -> CGPoint {
		CGPoint(x: scanner.scanDouble() ?? 0, y: scanner.scanDouble() ?? 0)
	}
	while !scanner.isAtEnd {
		guard let command = scanner.scanCharacter() else { break }
		switch command {
		case "M": path.move(to: point())
		case "L": path.addLine(to: point())
		case "C": let a = point(), b = point(), c = point(); path.addCurve(to: c, control1: a, control2: b)
		case "Z": path.closeSubpath()
		default: fatalError("unsupported path command \(command)")
		}
	}
	return path
}

// MARK: - Coin

/// The coin on a 64-unit grid centred on (32, 32), y down. Bootstrap's coin is
/// a 16-unit ring, inner ring and dollar sign; this keeps that idea, redraws the
/// dollar sign as a balanced, point-symmetric S, and sets the ring weights
/// between the wordmark's hairlines and stems.
struct CoinDesign {
	var outerRing: CGFloat = 5
	var innerRing: CGFloat? = 2
	var innerRadius: CGFloat = 23.5
	var sStroke: CGFloat = 4.8
	var sHalfHeight: CGFloat = 10.6
	var sHalfWidth: CGFloat = 6.6
	var bar: CGFloat = 3.3
	var barReach: CGFloat = 16.2
}

func circle(_ r: CGFloat, _ c: CGPoint = CGPoint(x: 32, y: 32)) -> CGPath {
	CGPath(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r), transform: nil)
}

func ring(outer: CGFloat, thickness: CGFloat) -> CGPath {
	circle(outer).subtracting(circle(outer - thickness))
}

func dollar(_ c: CoinDesign) -> CGPath {
	let h = c.sHalfHeight, w = c.sHalfWidth
	let top: CGFloat = 0.94
	func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
		let s = y > 0 ? top : 1
		return CGPoint(x: 32 + x * s, y: 32 - y * (y > 0 ? 1 : 1))
	}
	let spine = CGMutablePath()
	spine.move(to: p(w * 0.93, h * 0.6))
	spine.addCurve(to: p(0, h), control1: p(w * 0.8, h * 0.9), control2: p(w * 0.45, h))
	spine.addCurve(to: p(-w, h * 0.5), control1: p(-w * 0.56, h), control2: p(-w, h * 0.8))
	spine.addCurve(to: p(w, -h * 0.5), control1: p(-w, h * 0.06), control2: p(w, -h * 0.06))
	spine.addCurve(to: p(0, -h), control1: p(w, -h * 0.8), control2: p(w * 0.56, -h))
	spine.addCurve(to: p(-w * 0.93, -h * 0.6), control1: p(-w * 0.45, -h), control2: p(-w * 0.8, -h * 0.9))
	let s = spine.copy(strokingWithWidth: c.sStroke, lineCap: .butt, lineJoin: .round, miterLimit: 4)
	let overlap = c.sStroke / 2
	let topBar = CGPath(rect: CGRect(x: 32 - c.bar / 2, y: 32 - c.barReach, width: c.bar, height: c.barReach - h + overlap), transform: nil)
	let bottomBar = CGPath(rect: CGRect(x: 32 - c.bar / 2, y: 32 + h - overlap, width: c.bar, height: c.barReach - h + overlap), transform: nil)
	return s.union(topBar).union(bottomBar)
}

func coin(_ c: CoinDesign) -> CGPath {
	var shape = ring(outer: 32, thickness: c.outerRing)
	if let inner = c.innerRing { shape = shape.union(ring(outer: c.innerRadius + inner / 2, thickness: inner)) }
	return shape.union(dollar(c)).normalized()
}

let fullCoin = CoinDesign()
let smallCoin = CoinDesign(outerRing: 6.5, innerRing: nil, sStroke: 7, sHalfHeight: 11.5, sHalfWidth: 7.8, bar: 5, barReach: 17.6)
let coinPath = coin(fullCoin)
let coinRings = ring(outer: 32, thickness: fullCoin.outerRing).union(ring(outer: fullCoin.innerRadius + 1, thickness: 2)).normalized()
let coinDollar = dollar(fullCoin).normalized()
let smallCoinPath = coin(smallCoin)
let faviconPath = circle(32).subtracting(dollar(CoinDesign(sStroke: 7.4, sHalfHeight: 12.4, sHalfWidth: 8.6, bar: 5.4, barReach: 19))).normalized()

// MARK: - Wordmark

let traced = try String(contentsOf: brand.appending(path: "source/wordmark-traced.svg"), encoding: .utf8)
let tracedD = String(traced.firstMatch(of: #/ d="([^"]+)"/#)!.1)
let wordmark = parsePath(tracedD)
let wordBox = wordmark.boundingBoxOfPath
let capHeight: CGFloat = 700

func moved(_ path: CGPath, scale: CGFloat = 1, dx: CGFloat = 0, dy: CGFloat = 0) -> CGPath {
	var t = CGAffineTransform(translationX: dx, y: dy).scaledBy(x: scale, y: scale)
	return path.copy(using: &t)!
}

func coinAt(diameter: CGFloat, center: CGPoint, path: CGPath = coinPath) -> CGPath {
	let s = diameter / 64
	return moved(path, scale: s, dx: center.x - 32 * s, dy: center.y - 32 * s)
}

// MARK: - Assets

struct Artwork {
	let name: String
	let paths: [(id: String, path: CGPath)]
	var box: CGRect { paths.map { $0.path.boundingBoxOfPath }.reduce(CGRect.null) { $0.union($1) } }
	/// Clear space on every side, in the artwork's own units.
	let clearSpace: CGFloat
}

// Ryan's lockup sets the coin a little taller than the capitals with a quarter
// cap of air. Round forms read small, so the coin is 8% taller than the caps
// and centred on them; the V's slant opens the gap at mid-height, so the gap
// tightens to a fifth of the cap height.
let horizontalCoin: CGFloat = capHeight * 1.08
let horizontalGap: CGFloat = capHeight * 0.2
let horizontal = Artwork(
	name: "lockup-horizontal",
	paths: [
		("coin", coinAt(diameter: horizontalCoin, center: CGPoint(x: horizontalCoin / 2, y: -capHeight / 2))),
		("wordmark", moved(wordmark, dx: horizontalCoin + horizontalGap - wordBox.minX))
	],
	clearSpace: capHeight / 2
)

let stackedCoin: CGFloat = capHeight * 2
let stacked = Artwork(
	name: "lockup-stacked",
	paths: [
		("coin", coinAt(diameter: stackedCoin, center: CGPoint(x: wordBox.midX, y: -capHeight - capHeight * 0.42 - stackedCoin / 2))),
		("wordmark", wordmark)
	],
	clearSpace: capHeight / 2
)

let artworks: [Artwork] = [
	Artwork(name: "coin", paths: [("coin", coinPath)], clearSpace: 16),
	Artwork(name: "coin-small", paths: [("coin", smallCoinPath)], clearSpace: 16),
	Artwork(name: "wordmark", paths: [("wordmark", wordmark)], clearSpace: capHeight / 2),
	horizontal,
	stacked
]

struct Variant {
	let suffix: String
	let ink: String
	let background: String?
}

let variants = [
	Variant(suffix: "primary-on-light", ink: Palette.primary, background: nil),
	Variant(suffix: "white-on-primary", ink: Palette.white, background: Palette.primary),
	Variant(suffix: "white-on-dark", ink: Palette.white, background: Palette.dark),
	Variant(suffix: "black", ink: Palette.black, background: nil),
	Variant(suffix: "white", ink: Palette.white, background: nil)
]

func svg(_ art: Artwork, ink: String, background: String?) -> String {
	var box = art.box
	if background != nil { box = box.insetBy(dx: -art.clearSpace, dy: -art.clearSpace) }
	let viewBox = [box.minX, box.minY, box.width, box.height].map(number).joined(separator: " ")
	var body = ""
	if let background {
		body += "<rect x=\"\(number(box.minX))\" y=\"\(number(box.minY))\" width=\"\(number(box.width))\" height=\"\(number(box.height))\" fill=\"\(background)\"/>\n"
	}
	let fillAttribute = ink == "currentColor" ? "fill=\"currentColor\"" : "fill=\"\(ink)\""
	body += "<g \(fillAttribute)>\n"
	for (id, path) in art.paths { body += "<path id=\"\(id)\" d=\"\(svgPath(path))\"/>\n" }
	body += "</g>\n"
	return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"\(viewBox)\" role=\"img\" aria-label=\"Veritas\">\n<title>Veritas</title>\n\(body)</svg>\n"
}

func write(_ text: String, _ path: String) throws {
	let url = brand.appending(path: path)
	try fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
	try text.write(to: url, atomically: true, encoding: .utf8)
}

try? fm.removeItem(at: brand.appending(path: "svg"))
for art in artworks {
	try write(svg(art, ink: "currentColor", background: nil), "svg/\(art.name).svg")
	for v in variants { try write(svg(art, ink: v.ink, background: v.background), "svg/\(art.name)-\(v.suffix).svg") }
}

// MARK: - Rasterising

func render(svgFile: URL, width: Int, height: Int? = nil, background: NSColor? = nil, inset: CGFloat = 0) -> NSBitmapImageRep {
	let image = NSImage(contentsOf: svgFile)!
	let h = height ?? Int((CGFloat(width) * image.size.height / image.size.width).rounded())
	let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
	NSGraphicsContext.saveGraphicsState()
	NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
	NSGraphicsContext.current?.imageInterpolation = .high
	if let background { background.setFill(); NSRect(x: 0, y: 0, width: width, height: h).fill() }
	image.draw(in: NSRect(x: inset, y: inset, width: CGFloat(width) - 2 * inset, height: CGFloat(h) - 2 * inset))
	NSGraphicsContext.restoreGraphicsState()
	return rep
}

func savePNG(_ rep: NSBitmapImageRep, _ path: String) throws {
	let url = brand.appending(path: path)
	try fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
	try rep.representation(using: .png, properties: [:])!.write(to: url)
}

// MARK: - Small sizes

func squareSVG(_ path: CGPath, fill: String, size: CGFloat = 64) -> String {
	"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 \(number(size)) \(number(size))\">\n<path fill=\"\(fill)\" d=\"\(svgPath(path))\"/>\n</svg>\n"
}

try? fm.removeItem(at: brand.appending(path: "favicon"))
try write(squareSVG(faviconPath, fill: Palette.primary), "favicon/favicon.svg")
var icoImages: [Data] = []
for size in [16, 32, 48, 180, 512] {
	let rep = render(svgFile: brand.appending(path: "favicon/favicon.svg"), width: size)
	try savePNG(rep, "favicon/favicon-\(size).png")
	if size <= 48 { icoImages.append(rep.representation(using: .png, properties: [:])!) }
}

/// An ICO container holding PNG frames, which every current browser reads.
func ico(_ frames: [Data], sizes: [Int]) -> Data {
	var data = Data()
	func le16(_ v: Int) { data.append(contentsOf: [UInt8(v & 0xff), UInt8(v >> 8 & 0xff)]) }
	func le32(_ v: Int) { le16(v & 0xffff); le16(v >> 16 & 0xffff) }
	le16(0); le16(1); le16(frames.count)
	var offset = 6 + 16 * frames.count
	for (frame, size) in zip(frames, sizes) {
		data.append(contentsOf: [UInt8(size % 256), UInt8(size % 256), 0, 0])
		le16(1); le16(32); le32(frame.count); le32(offset)
		offset += frame.count
	}
	frames.forEach { data.append($0) }
	return data
}
try ico(icoImages, sizes: [16, 32, 48]).write(to: brand.appending(path: "favicon/favicon.ico"))

// 16 and 32 px coins: no inner ring, a heavier $.
try? fm.removeItem(at: brand.appending(path: "small"))
for size in [16, 32] {
	for (suffix, ink) in [("primary", Palette.primary), ("black", Palette.black), ("white", Palette.white)] {
		try write(squareSVG(smallCoinPath, fill: ink), "small/coin-\(size)-\(suffix).svg")
		for scale in [1, 2] {
			let rep = render(svgFile: brand.appending(path: "small/coin-\(size)-\(suffix).svg"), width: size * scale)
			try savePNG(rep, "small/coin-\(size)-\(suffix)\(scale == 2 ? "@2x" : "").png")
		}
	}
}

// Menu bar: a 16 pt coin in an 18 pt template image, black on clear.
try? fm.removeItem(at: brand.appending(path: "menubar"))
let menubarPath = moved(smallCoinPath, scale: 16 / 64, dx: 1, dy: 1)
try write(squareSVG(menubarPath, fill: Palette.black, size: 18), "menubar/MenuBarTemplate.svg")
for scale in [1, 2, 3] {
	let rep = render(svgFile: brand.appending(path: "menubar/MenuBarTemplate.svg"), width: 18 * scale)
	try savePNG(rep, "menubar/MenuBarTemplate\(scale == 1 ? "" : "@\(scale)x").png")
}

// MARK: - App icon (Icon Composer)

// Liquid Glass icons are layers on a 1024 canvas; the system draws the squircle,
// the glass and the light, dark, clear and tinted appearances from them. The
// rings sit behind the $ so the glass gives the coin real depth.
let iconCoinDiameter: CGFloat = 620
let iconScale = iconCoinDiameter / 64
let iconOffset = (1024 - iconCoinDiameter) / 2
func iconLayer(_ path: CGPath) -> String {
	squareSVG(moved(path, scale: iconScale, dx: iconOffset, dy: iconOffset), fill: Palette.white, size: 1024)
}

func srgb(_ hex: String) -> String {
	let v = UInt32(hex.dropFirst(), radix: 16)!
	let parts = [v >> 16 & 0xff, v >> 8 & 0xff, v & 0xff].map { String(format: "%.5f", Double($0) / 255) }
	return "srgb:\(parts.joined(separator: ",")),1.00000"
}

let iconJSON = """
{
  "fill" : {
    "automatic-gradient" : "\(srgb(Palette.primary))"
  },
  "fill-specializations" : [
    {
      "appearance" : "dark",
      "value" : {
        "automatic-gradient" : "\(srgb("#1B1A24"))"
      }
    }
  ],
  "groups" : [
    {
      "layers" : [
        {
          "fill-specializations" : [
            {
              "appearance" : "dark",
              "value" : {
                "solid" : "\(srgb(Palette.primaryDarkMode))"
              }
            }
          ],
          "glass" : true,
          "image-name" : "2-dollar.svg",
          "name" : "2-dollar"
        }
      ],
      "shadow" : {
        "kind" : "neutral",
        "opacity" : 0.5
      },
      "translucency" : {
        "enabled" : true,
        "value" : 0.2
      }
    },
    {
      "layers" : [
        {
          "fill-specializations" : [
            {
              "appearance" : "dark",
              "value" : {
                "solid" : "\(srgb(Palette.primaryDarkMode))"
              }
            }
          ],
          "glass" : true,
          "image-name" : "1-rings.svg",
          "name" : "1-rings"
        }
      ],
      "shadow" : {
        "kind" : "neutral",
        "opacity" : 0.5
      },
      "translucency" : {
        "enabled" : true,
        "value" : 0.3
      }
    }
  ],
  "supported-platforms" : {
    "squares" : [
      "macOS"
    ]
  }
}

"""

let iconBundle = brand.appending(path: "app-icon/AppIcon.icon")
try? fm.removeItem(at: iconBundle)
try write(iconLayer(coinRings), "app-icon/AppIcon.icon/Assets/1-rings.svg")
try write(iconLayer(coinDollar), "app-icon/AppIcon.icon/Assets/2-dollar.svg")
try write(iconJSON, "app-icon/AppIcon.icon/icon.json")

let appIcon = root.appending(path: "apps/macos/Veritas/AppIcon.icon")
try? fm.removeItem(at: appIcon)
try fm.copyItem(at: iconBundle, to: appIcon)

let ictool = URL(fileURLWithPath: "/Applications/Xcode.app/Contents/Applications/Icon Composer.app/Contents/Executables/ictool")
func exportIcon(rendition: String, size: Int, scale: Int, to path: String) throws {
	let out = brand.appending(path: path)
	try fm.createDirectory(at: out.deletingLastPathComponent(), withIntermediateDirectories: true)
	let process = Process()
	process.executableURL = ictool
	process.arguments = [iconBundle.path, "--export-image", "--output-file", out.path, "--platform", "macOS", "--rendition", rendition,
		"--width", "\(size)", "--height", "\(size)", "--scale", "\(scale)"]
	try process.run()
	process.waitUntilExit()
	guard process.terminationStatus == 0, fm.fileExists(atPath: out.path) else { throw CocoaError(.fileWriteUnknown) }
}

try? fm.removeItem(at: brand.appending(path: "app-icon/png"))
let renditions = ["Default", "Dark", "ClearLight", "ClearDark", "TintedLight", "TintedDark"]
if fm.fileExists(atPath: ictool.path) {
	for size in [16, 32, 128, 256, 512] {
		for scale in [1, 2] {
			try exportIcon(rendition: "Default", size: size, scale: scale, to: "app-icon/png/icon_\(size)x\(size)\(scale == 2 ? "@2x" : "").png")
		}
	}
	for rendition in renditions {
		try exportIcon(rendition: rendition, size: 256, scale: 2, to: "app-icon/png/appearance-\(rendition.lowercased()).png")
	}
} else {
	print("Icon Composer's ictool is missing; skipped the app icon PNGs")
}

// MARK: - Swift artwork

let swift = """
// Generated by tools/brand/build-brand.swift. Edit the generator, not this file.

enum BrandArtwork {
	static let coinViewBox = CGRect(x: 0, y: 0, width: 64, height: 64)
	static let coin = "\(svgPath(coinPath))"
	static let smallCoin = "\(svgPath(smallCoinPath))"
	static let dollar = "\(svgPath(coinDollar))"
	static let outerRing = (radius: CGFloat(\(number(32 - fullCoin.outerRing / 2))), width: CGFloat(\(number(fullCoin.outerRing))))
	static let innerRing = (radius: CGFloat(\(number(fullCoin.innerRadius))), width: CGFloat(\(number(fullCoin.innerRing ?? 0))))
	static let wordmarkViewBox = CGRect(x: \(number(wordBox.minX)), y: \(number(wordBox.minY)), width: \(number(wordBox.width)), height: \(number(wordBox.height)))
	static let wordmark = "\(svgPath(wordmark))"
	static let capHeight: CGFloat = \(number(capHeight))
	static let lockupCoinDiameter: CGFloat = \(number(horizontalCoin))
	static let lockupGap: CGFloat = \(number(horizontalGap))
}

"""
try ("import CoreGraphics\n\n" + swift).write(
	to: root.appending(path: "apps/macos/Packages/VeritasKit/Sources/VeritasDesign/BrandArtwork.swift"),
	atomically: true, encoding: .utf8
)

// MARK: - Previews

func hex(_ s: String) -> NSColor {
	let v = UInt32(s.dropFirst(), radix: 16)!
	return NSColor(srgbRed: CGFloat(v >> 16 & 0xff) / 255, green: CGFloat(v >> 8 & 0xff) / 255, blue: CGFloat(v & 0xff) / 255, alpha: 1)
}

func label(_ text: String, at point: NSPoint, size: CGFloat = 22, color: NSColor = hex("#55555E")) {
	(text as NSString).draw(at: point, withAttributes: [.font: NSFont.systemFont(ofSize: size, weight: .medium), .foregroundColor: color])
}

func sheet(width: Int, height: Int, draw: () -> Void) -> NSBitmapImageRep {
	let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
	NSGraphicsContext.saveGraphicsState()
	let cg = NSGraphicsContext(bitmapImageRep: rep)!.cgContext
	cg.translateBy(x: 0, y: CGFloat(height))
	cg.scaleBy(x: 1, y: -1)
	let context = NSGraphicsContext(cgContext: cg, flipped: true)
	NSGraphicsContext.current = context
	context.imageInterpolation = .high
	hex("#F4F4F6").setFill()
	NSRect(x: 0, y: 0, width: width, height: height).fill()
	draw()
	NSGraphicsContext.restoreGraphicsState()
	return rep
}

func drawSVG(_ path: String, in rect: NSRect) {
	let image = NSImage(contentsOf: brand.appending(path: path))!
	let fit = min(rect.width / image.size.width, rect.height / image.size.height)
	let size = NSSize(width: image.size.width * fit, height: image.size.height * fit)
	image.draw(in: NSRect(x: rect.midX - size.width / 2, y: rect.midY - size.height / 2, width: size.width, height: size.height),
		from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
}

let tile = NSSize(width: 520, height: 300)
let columns = variants.count
let rowNames = ["lockup-horizontal", "lockup-stacked", "wordmark", "coin", "coin-small"]
let margin: CGFloat = 60
let sheetWidth = Int(margin * 2 + CGFloat(columns) * tile.width + CGFloat(columns - 1) * 20)
let smallRowHeight: CGFloat = 260
let iconRowHeight: CGFloat = 340
let sheetHeight = Int(margin + 70 + CGFloat(rowNames.count) * (tile.height + 60) + smallRowHeight + iconRowHeight + margin + 60)
let preview = sheet(width: sheetWidth, height: sheetHeight) {
	label("Veritas brand", at: NSPoint(x: margin, y: margin - 10), size: 34, color: hex("#1C1C21"))
	var y = margin + 70
	for name in rowNames {
		label(name, at: NSPoint(x: margin, y: y))
		y += 36
		for (i, v) in variants.enumerated() {
			let rect = NSRect(x: margin + CGFloat(i) * (tile.width + 20), y: y, width: tile.width, height: tile.height - 36)
			let checker = v.ink == Palette.white && v.background == nil
			(checker ? hex("#3A3A44") : hex("#FFFFFF")).setFill()
			NSBezierPath(roundedRect: rect, xRadius: 14, yRadius: 14).fill()
			drawSVG("svg/\(name)-\(v.suffix).svg", in: rect.insetBy(dx: v.background == nil ? 50 : 24, dy: v.background == nil ? 40 : 24))
			if name == rowNames.first { label(v.suffix, at: NSPoint(x: rect.minX, y: rect.maxY + 2), size: 18) }
		}
		y += tile.height + 24
	}
	label("16 px and 32 px coins, favicon, menu bar template, enlarged with hard pixels", at: NSPoint(x: margin, y: y))
	y += 40
	var x = margin
	for file in ["small/coin-16-primary.png", "small/coin-32-primary.png", "small/coin-16-primary@2x.png", "small/coin-32-primary@2x.png",
		"favicon/favicon-16.png", "favicon/favicon-32.png", "favicon/favicon-48.png", "menubar/MenuBarTemplate.png", "menubar/MenuBarTemplate@2x.png"] {
		let image = NSImage(contentsOf: brand.appending(path: file))!
		let pixels = CGFloat(image.representations.first!.pixelsWide)
		let box = NSRect(x: x, y: y, width: 150, height: 150)
		hex("#FFFFFF").setFill()
		NSBezierPath(roundedRect: box, xRadius: 12, yRadius: 12).fill()
		let zoom = max(1, (128 / pixels).rounded(.down))
		let side = pixels * zoom
		NSGraphicsContext.current?.imageInterpolation = .none
		image.draw(in: NSRect(x: box.midX - side / 2, y: box.midY - side / 2, width: side, height: side), from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
		NSGraphicsContext.current?.imageInterpolation = .high
		label("\(Int(pixels)) px, shown \(Int(zoom))×", at: NSPoint(x: box.minX + 4, y: box.maxY + 4), size: 15)
		x += 170
	}
	y += smallRowHeight - 40
	label("App icon: default, dark, clear light, clear dark, tinted light, tinted dark", at: NSPoint(x: margin, y: y))
	y += 40
	for (i, rendition) in renditions.enumerated() {
		let file = brand.appending(path: "app-icon/png/appearance-\(rendition.lowercased()).png")
		guard let image = NSImage(contentsOf: file) else { continue }
		let box = NSRect(x: margin + CGFloat(i) * 300, y: y, width: 270, height: 270)
		(rendition.hasSuffix("Dark") || rendition == "Dark" ? hex("#2A2A30") : hex("#DCDCE2")).setFill()
		NSBezierPath(roundedRect: box, xRadius: 16, yRadius: 16).fill()
		image.draw(in: box.insetBy(dx: 10, dy: 10), from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
	}
}
try savePNG(preview, "preview.png")

// The trace beside Ryan's PNG, and the trace drawn over it.
let sourcePNG = NSImage(contentsOf: root.appending(path: "brand/Veritas.png"))!
let sourceRep = sourcePNG.representations.first!
let pngScale = CGFloat(sourceRep.pixelsWide) / sourcePNG.size.width
func traceMeta(_ key: String) -> CGFloat {
	let start = traced.range(of: "data-\(key)=\"")!.upperBound
	let end = traced[start...].firstIndex(of: "\"")!
	return CGFloat(Double(traced[start..<end])!)
}
let tracedLeft = traceMeta("left"), tracedBaseline = traceMeta("baseline"), tracedCap = traceMeta("cap")
let cropOrigin = CGPoint(x: (tracedLeft - 6).rounded(), y: (tracedBaseline - tracedCap - 6).rounded())
let cropSize = CGSize(width: 340, height: 85)
let sourceCrop = NSRect(x: cropOrigin.x / pngScale, y: (CGFloat(sourceRep.pixelsHigh) - cropOrigin.y - cropSize.height) / pngScale, width: cropSize.width / pngScale, height: cropSize.height / pngScale)
let comparison = sheet(width: 2160, height: 1850) {
	let target = NSRect(x: 60, y: 90, width: 2040, height: 2040 * cropSize.height / cropSize.width)
	label("Ryan's PNG (Canva, ITC Benguiat), enlarged 6×", at: NSPoint(x: 60, y: 40))
	hex("#FFFFFF").setFill(); target.fill()
	NSGraphicsContext.current?.imageInterpolation = .high
	sourcePNG.draw(in: target, from: sourceCrop, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
	let lower = target.offsetBy(dx: 0, dy: target.height + 110)
	label("Traced outline (wordmark.svg) drawn over the PNG in red", at: NSPoint(x: 60, y: lower.minY - 50))
	hex("#FFFFFF").setFill(); lower.fill()
	sourcePNG.draw(in: lower, from: sourceCrop, operation: .sourceOver, fraction: 0.55, respectFlipped: true, hints: nil)
	if let ctx = NSGraphicsContext.current?.cgContext {
		let pixels = lower.width / cropSize.width
		let scale = pixels * tracedCap / capHeight
		ctx.saveGState()
		ctx.translateBy(x: lower.minX + (tracedLeft - cropOrigin.x) * pixels, y: lower.minY + (tracedBaseline - cropOrigin.y) * pixels)
		ctx.scaleBy(x: scale, y: scale)
		ctx.addPath(wordmark)
		ctx.setStrokeColor(NSColor.systemRed.cgColor)
		ctx.setLineWidth(2 / scale)
		ctx.strokePath()
		ctx.restoreGState()
		let clean = lower.offsetBy(dx: 0, dy: lower.height + 110)
		label("wordmark.svg on its own", at: NSPoint(x: 60, y: clean.minY - 50))
		hex("#FFFFFF").setFill(); clean.fill()
		ctx.saveGState()
		ctx.translateBy(x: clean.minX + (tracedLeft - cropOrigin.x) * pixels, y: clean.minY + (tracedBaseline - cropOrigin.y) * pixels)
		ctx.scaleBy(x: scale, y: scale)
		ctx.addPath(wordmark)
		ctx.setFillColor(hex(Palette.primary).cgColor)
		ctx.fillPath()
		ctx.restoreGState()
	}
}
try savePNG(comparison, "trace-comparison.png")

print("Built \(artworks.count * (variants.count + 1)) SVGs, favicons, menu bar template, app icon and previews in design/brand")
