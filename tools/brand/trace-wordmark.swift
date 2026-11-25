// Traces the "Veritas" wordmark out of Ryan's logo PNG into smooth cubic outlines.
//
//     swift tools/brand/trace-wordmark.swift [--overlay out.png]
//
// Reads brand/Veritas@4x.png when it exists, else brand/Veritas.png, and writes
// design/brand/source/wordmark-traced.svg. The pipeline: crop the wordmark, upscale
// it with Lanczos, threshold, find contours with Vision, then fit each contour with
// cubic Béziers anchored at corners and at x/y extrema (the way type is drawn).

import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import Vision

struct Point {
	var x: Double
	var y: Double
	static func + (a: Point, b: Point) -> Point { Point(x: a.x + b.x, y: a.y + b.y) }
	static func - (a: Point, b: Point) -> Point { Point(x: a.x - b.x, y: a.y - b.y) }
	static func * (a: Point, s: Double) -> Point { Point(x: a.x * s, y: a.y * s) }
	var length: Double { (x * x + y * y).squareRoot() }
	var normalized: Point { let l = length; return l == 0 ? self : Point(x: x / l, y: y / l) }
	func dot(_ o: Point) -> Double { x * o.x + y * o.y }
}

struct Cubic {
	var p0: Point, p1: Point, p2: Point, p3: Point
	func at(_ t: Double) -> Point {
		let mt = 1 - t
		return p0 * (mt * mt * mt) + p1 * (3 * mt * mt * t) + p2 * (3 * mt * t * t) + p3 * (t * t * t)
	}
	func d1(_ t: Double) -> Point {
		let mt = 1 - t
		return (p1 - p0) * (3 * mt * mt) + (p2 - p1) * (6 * mt * t) + (p3 - p2) * (3 * t * t)
	}
	func d2(_ t: Double) -> Point {
		(p2 - p1 * 2 + p0) * (6 * (1 - t)) + (p3 - p2 * 2 + p1) * (6 * t)
	}
	var isStraight: Bool {
		let chord = p3 - p0
		let length = chord.length
		guard length > 0 else { return true }
		func offset(_ p: Point) -> Double { abs((p - p0).x * chord.y - (p - p0).y * chord.x) / length }
		return offset(p1) < 0.6 && offset(p2) < 0.6
	}
}

// MARK: - Image

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let hiRes = root.appending(path: "brand/Veritas@4x.png")
let source = FileManager.default.fileExists(atPath: hiRes.path) ? hiRes : root.appending(path: "brand/Veritas.png")
let sourceScale: Double = source == hiRes ? 4 : 1
guard let rep = NSBitmapImageRep(data: try Data(contentsOf: source)) else { fatalError("can't read \(source.path)") }

let width = rep.pixelsWide, height = rep.pixelsHigh
var coverage = [Double](repeating: 0, count: width * height)
let inkGreen = 22.0
for y in 0..<height {
	for x in 0..<width {
		guard let color = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
		let green = color.greenComponent * 255 * color.alphaComponent + 255 * (1 - color.alphaComponent)
		coverage[y * width + x] = min(1, max(0, (255 - green) / (255 - inkGreen)))
	}
}

func inkColumns() -> [ClosedRange<Int>] {
	var runs: [ClosedRange<Int>] = []
	var start: Int?
	for x in 0..<width {
		let hasInk = (0..<height).contains { coverage[$0 * width + x] > 0.15 }
		if hasInk, start == nil { start = x }
		if !hasInk, let s = start { runs.append(s...(x - 1)); start = nil }
	}
	if let s = start { runs.append(s...(width - 1)) }
	return runs
}

let runs = inkColumns()
let gaps = zip(runs, runs.dropFirst()).map { $1.lowerBound - $0.upperBound }
let coinEnd = gaps.firstIndex(of: gaps.max()!)!
let wordRuns = runs[(coinEnd + 1)...]
let pad = Int(4 * sourceScale)
let cropX = wordRuns.first!.lowerBound - pad
let cropRight = wordRuns.last!.upperBound + pad
let inkRows = (0..<height).filter { y in (cropX...cropRight).contains { coverage[y * width + $0] > 0.15 } }
let cropY = inkRows.first! - pad
let cropW = cropRight - cropX + 1
let cropH = inkRows.last! + pad - cropY + 1

let upscale = 8.0 / sourceScale
func grayImage(_ values: (Int, Int) -> Double, _ w: Int, _ h: Int) -> CGImage {
	var bytes = [UInt8](repeating: 0, count: w * h)
	for y in 0..<h { for x in 0..<w { bytes[y * w + x] = UInt8(max(0, min(255, (1 - values(x, y)) * 255)).rounded()) } }
	let provider = CGDataProvider(data: Data(bytes) as CFData)!
	return CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: w, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: 0), provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
}

let cropped = grayImage({ coverage[($1 + cropY) * width + $0 + cropX] }, cropW, cropH)
let context = CIContext(options: [.workingColorSpace: NSNull()])
let lanczos = CIFilter.lanczosScaleTransform()
lanczos.inputImage = CIImage(cgImage: cropped)
lanczos.scale = Float(upscale)
lanczos.aspectRatio = 1
let threshold = CIFilter.colorThreshold()
threshold.inputImage = lanczos.outputImage!
threshold.threshold = 0.5
let upW = Int((Double(cropW) * upscale).rounded()), upH = Int((Double(cropH) * upscale).rounded())
let binary = context.createCGImage(threshold.outputImage!, from: CGRect(x: 0, y: 0, width: upW, height: upH))!
let smoothUp = context.createCGImage(lanczos.outputImage!, from: CGRect(x: 0, y: 0, width: upW, height: upH))!

// MARK: - Contours

let request = VNDetectContoursRequest()
request.detectsDarkOnLight = true
request.contrastAdjustment = 1
request.maximumImageDimension = max(upW, upH)
try VNImageRequestHandler(cgImage: binary).perform([request])
guard let observation = request.results?.first else { fatalError("no contours") }

func pixels(_ contour: VNContour) -> [Point] {
	contour.normalizedPoints.map { Point(x: Double($0.x) * Double(upW), y: (1 - Double($0.y)) * Double(upH)) }
}

var polygons: [(points: [Point], isHole: Bool)] = []
func collect(_ contour: VNContour, isHole: Bool) {
	let points = pixels(contour)
	if points.count > 24 { polygons.append((points, isHole)) }
	for child in contour.childContours { collect(child, isHole: !isHole) }
}
observation.topLevelContours.forEach { collect($0, isHole: false) }

// MARK: - Fitting

func signedArea(_ p: [Point]) -> Double {
	var sum = 0.0
	for i in p.indices { let a = p[i], b = p[(i + 1) % p.count]; sum += a.x * b.y - b.x * a.y }
	return sum / 2
}

func resample(_ p: [Point], spacing: Double) -> [Point] {
	var result = [p[0]]
	var carry = 0.0
	for i in 0..<p.count {
		let a = p[i], b = p[(i + 1) % p.count]
		let segment = (b - a).length
		var t = spacing - carry
		while t <= segment {
			result.append(a + (b - a) * (t / segment))
			t += spacing
		}
		carry = segment - (t - spacing)
	}
	if let last = result.last, (last - result[0]).length < spacing * 0.5 { result.removeLast() }
	return result
}

func smooth(_ p: [Point], sigma: Double, pinned: Set<Int>) -> [Point] {
	let radius = Int(sigma * 3)
	let weights = (-radius...radius).map { exp(-Double($0 * $0) / (2 * sigma * sigma)) }
	let n = p.count
	return p.indices.map { i in
		if pinned.contains(i) { return p[i] }
		// Never blur across a corner: shrink the window to the nearest pinned point.
		var reach = radius
		for d in 1...radius where pinned.contains((i + d) % n) || pinned.contains((i - d + n) % n) { reach = d - 1; break }
		guard reach > 0 else { return p[i] }
		var sum = Point(x: 0, y: 0), total = 0.0
		for d in -reach...reach { let w = weights[d + radius]; sum = sum + p[(i + d + n) % n] * w; total += w }
		return sum * (1 / total)
	}
}

func turning(_ p: [Point], _ i: Int, _ k: Int) -> Double {
	let n = p.count
	let a = (p[i] - p[(i - k + n) % n]).normalized, b = (p[(i + k) % n] - p[i]).normalized
	return acos(max(-1, min(1, a.dot(b))))
}

func corners(_ p: [Point], k: Int, minAngle: Double) -> [Int] {
	let n = p.count
	let angles = p.indices.map { turning(p, $0, k) }
	var result: [Int] = []
	// A true corner turns as sharply up close as from afar; a tight curve does not.
	for i in p.indices where angles[i] > minAngle && turning(p, i, 3) > angles[i] * 0.7 {
		let window = (1...k).flatMap { [angles[(i + $0) % n], angles[(i - $0 + n) % n]] }
		if window.allSatisfy({ angles[i] >= $0 }), !result.contains(where: { abs($0 - i) <= k }) { result.append(i) }
	}
	return result.sorted()
}

func chordParameters(_ p: ArraySlice<Point>) -> [Double] {
	var u = [0.0]
	var previous = p.first!
	for q in p.dropFirst() { u.append(u.last! + (q - previous).length); previous = q }
	let total = u.last!
	return total == 0 ? u : u.map { $0 / total }
}

func generate(_ p: ArraySlice<Point>, _ u: [Double], _ t1: Point, _ t2: Point) -> Cubic {
	let first = p.first!, last = p.last!
	var c00 = 0.0, c01 = 0.0, c11 = 0.0, x0 = 0.0, x1 = 0.0
	for (i, q) in p.enumerated() {
		let t = u[i], mt = 1 - t
		let b0 = mt * mt * mt, b1 = 3 * t * mt * mt, b2 = 3 * t * t * mt, b3 = t * t * t
		let a1 = t1 * b1, a2 = t2 * b2
		c00 += a1.dot(a1); c01 += a1.dot(a2); c11 += a2.dot(a2)
		let rest = q - (first * (b0 + b1) + last * (b2 + b3))
		x0 += a1.dot(rest); x1 += a2.dot(rest)
	}
	let det = c00 * c11 - c01 * c01
	var alpha1 = det == 0 ? 0 : (x0 * c11 - x1 * c01) / det
	var alpha2 = det == 0 ? 0 : (c00 * x1 - c01 * x0) / det
	let span = (last - first).length
	if alpha1 < span * 1e-3 || alpha2 < span * 1e-3 { alpha1 = span / 3; alpha2 = span / 3 }
	return Cubic(p0: first, p1: first + t1 * alpha1, p2: last + t2 * alpha2, p3: last)
}

func maxError(_ c: Cubic, _ p: ArraySlice<Point>, _ u: [Double]) -> (Double, Int) {
	var worst = 0.0, index = p.startIndex + p.count / 2
	for (i, q) in p.enumerated() {
		let d = (c.at(u[i]) - q).length
		if d > worst { worst = d; index = p.startIndex + i }
	}
	return (worst, index)
}

func reparameterize(_ c: Cubic, _ p: ArraySlice<Point>, _ u: [Double]) -> [Double] {
	zip(p, u).map { q, t in
		let d = c.at(t) - q, d1 = c.d1(t), d2 = c.d2(t)
		let denominator = d1.dot(d1) + d.dot(d2)
		return denominator == 0 ? t : min(1, max(0, t - d.dot(d1) / denominator))
	}
}

func fit(_ p: ArraySlice<Point>, _ t1: Point, _ t2: Point, tolerance: Double) -> [Cubic] {
	if p.count <= 3 {
		let span = (p.last! - p.first!).length / 3
		return [Cubic(p0: p.first!, p1: p.first! + t1 * span, p2: p.last! + t2 * span, p3: p.last!)]
	}
	var u = chordParameters(p)
	var curve = generate(p, u, t1, t2)
	var (error, split) = maxError(curve, p, u)
	if error < tolerance { return [curve] }
	if error < tolerance * 4 {
		for _ in 0..<20 {
			u = reparameterize(curve, p, u)
			curve = generate(p, u, t1, t2)
			(error, split) = maxError(curve, p, u)
			if error < tolerance { return [curve] }
		}
	}
	split = min(max(split, p.startIndex + 1), p.endIndex - 2)
	let center = (p[split - 1] - p[split + 1]).normalized
	return fit(p[p.startIndex...split], t1, center, tolerance: tolerance) + fit(p[split...], center * -1, t2, tolerance: tolerance)
}

/// x/y extrema between two corners, where type designers put their nodes.
func extrema(_ p: [Point], from a: Int, to b: Int, keepClear: Int) -> [(Int, Point)] {
	let n = p.count
	let length = (b - a + n) % n == 0 ? n : (b - a + n) % n
	guard length > keepClear * 2 + 2 else { return [] }
	var result: [(Int, Point)] = []
	let lookahead = 4
	for step in keepClear..<(length - keepClear) {
		let i = (a + step) % n
		let previous = p[(i - lookahead + n) % n], next = p[(i + lookahead) % n], here = p[i]
		let isXExtreme = (here.x - previous.x) * (next.x - here.x) < 0 && abs(here.x - previous.x) + abs(next.x - here.x) > 0.05
		let isYExtreme = (here.y - previous.y) * (next.y - here.y) < 0 && abs(here.y - previous.y) + abs(next.y - here.y) > 0.05
		let direction = (next - previous)
		if isXExtreme, abs(direction.y) > abs(direction.x) * 2 {
			result.append((i, Point(x: 0, y: direction.y > 0 ? 1 : -1)))
		} else if isYExtreme, abs(direction.x) > abs(direction.y) * 2 {
			result.append((i, Point(x: direction.x > 0 ? 1 : -1, y: 0)))
		}
	}
	var thinned: [(Int, Point)] = []
	for item in result where !thinned.contains(where: { abs($0.0 - item.0) < 40 }) { thinned.append(item) }
	return thinned
}

func deviation(_ run: [Point]) -> Double {
	let a = run[0], chord = run[run.count - 1] - a
	let length = chord.length
	guard length > 0 else { return .infinity }
	return run.map { abs(($0 - a).x * chord.y - ($0 - a).y * chord.x) / length }.max()!
}

struct Anchor {
	let index: Int
	/// Direction of travel through a smooth node; nil at a corner.
	var tangent: Point?
	var lineToNext = false
}

func run(_ points: [Point], from a: Int, to b: Int) -> [Point] {
	let n = points.count
	var result: [Point] = []
	var i = a
	repeat { result.append(points[i]); i = (i + 1) % n } while i != b
	result.append(points[b])
	return result
}

/// Long stretches that are straight within `straightness`: stems, crossbars, serif
/// faces. Their ends become nodes whose handles continue the line, so a line
/// flows into the curve after it without a kink.
func straightStretches(_ points: [Point], corners: [Int], minLength: Int) -> [(Int, Int)] {
	let n = points.count
	let cornerSet = Set(corners)
	var stretches: [(Int, Int)] = []
	var covered = Set<Int>()
	var start = corners.first ?? 0
	var walked = 0
	while walked < n {
		var end = start
		var length = 0
		while length < n - 1 {
			let next = (end + 1) % n
			if length > 0, cornerSet.contains(end) { break }
			if covered.contains(next) { break }
			let candidate = run(points, from: start, to: next)
			if candidate.count > 3, deviation(candidate) > straightness { break }
			end = next
			length += 1
		}
		if length >= minLength {
			stretches.append((start, end))
			for k in 0...length { covered.insert((start + k) % n) }
			walked += length
			start = end
		} else {
			start = (start + 1) % n
			walked += 1
		}
	}
	return stretches
}

func trace(_ raw: [Point], isHole: Bool, tolerance: Double) -> [Cubic] {
	var points = resample(raw, spacing: 1)
	if (signedArea(points) < 0) != isHole { points.reverse() }
	let n = points.count
	let corner = corners(smooth(points, sigma: 1.5, pinned: []), k: 10, minAngle: 0.7)
	points = smooth(points, sigma: 6, pinned: Set(corner))

	var anchors: [Int: Anchor] = [:]
	for c in corner { anchors[c] = Anchor(index: c, tangent: nil) }
	let stretches = straightStretches(points, corners: corner, minLength: 90)
	var onLine = Set<Int>()
	for (s, e) in stretches {
		let direction = (points[e] - points[s]).normalized
		anchors[s, default: Anchor(index: s, tangent: direction)].lineToNext = true
		if anchors[e] == nil { anchors[e] = Anchor(index: e, tangent: direction) }
		var i = s
		while i != e { onLine.insert(i); i = (i + 1) % n }
		onLine.insert(e)
	}
	func isClear(_ i: Int) -> Bool {
		(-14...14).allSatisfy { !onLine.contains((i + $0 + n) % n) && anchors[(i + $0 + n) % n] == nil }
	}
	let ordered = anchors.keys.sorted()
	if ordered.isEmpty {
		for (i, t) in extrema(points, from: 0, to: 0, keepClear: 6) { anchors[i] = Anchor(index: i, tangent: t) }
	} else {
		for (j, a) in ordered.enumerated() where anchors[a]?.lineToNext != true {
			let b = ordered[(j + 1) % ordered.count]
			for (i, t) in extrema(points, from: a, to: b, keepClear: 10) where isClear(i) { anchors[i] = Anchor(index: i, tangent: t) }
		}
	}
	if anchors.isEmpty { anchors[0] = Anchor(index: 0, tangent: nil) }

	let sorted = anchors.keys.sorted().map { anchors[$0]! }
	var cubics: [Cubic] = []
	for (j, a) in sorted.enumerated() {
		let b = sorted[(j + 1) % sorted.count]
		let stretch = run(points, from: a.index, to: b.index)
		if a.lineToNext {
			let chord = stretch[stretch.count - 1] - stretch[0]
			cubics.append(Cubic(p0: stretch[0], p1: stretch[0] + chord * (1.0 / 3), p2: stretch[0] + chord * (2.0 / 3), p3: stretch[stretch.count - 1]))
			continue
		}
		let lead = min(4, stretch.count - 1)
		let t1 = a.tangent ?? (stretch[lead] - stretch[0]).normalized
		let t2 = b.tangent.map { $0 * -1 } ?? (stretch[stretch.count - 1 - lead] - stretch[stretch.count - 1]).normalized
		cubics += fit(stretch[...], t1, t2, tolerance: tolerance)
	}
	return withoutSlivers(cubics)
}

/// Twin nodes a hair apart read as a nick at large sizes; fold each sliver into the segment before it.
func withoutSlivers(_ cubics: [Cubic]) -> [Cubic] {
	var result: [Cubic] = []
	for c in cubics {
		if (c.p3 - c.p0).length < 4, var previous = result.popLast() {
			let shift = c.p3 - previous.p3
			previous.p2 = previous.p2 + shift
			previous.p3 = c.p3
			result.append(previous)
		} else {
			result.append(c)
		}
	}
	return result
}

// MARK: - Output

let tolerance = 3.0
let straightness = 1.6
let contours = polygons.map { trace($0.points, isHole: $0.isHole, tolerance: tolerance) }

/// Flat feet and flat tops sit exactly on the baseline and cap height; round
/// strokes keep their overshoot.
func dominantFlat(_ contours: [[Cubic]], near target: Double, window: Double) -> Double {
	var histogram: [Int: Double] = [:]
	for c in contours.flatMap({ $0 }) where c.isStraight && abs(c.p3.y - c.p0.y) < 0.8 && abs(c.p0.y - target) < window {
		histogram[Int(((c.p0.y + c.p3.y) / 2).rounded()), default: 0] += abs(c.p3.x - c.p0.x)
	}
	return Double(histogram.max { $0.value < $1.value }?.key ?? Int(target))
}

let allPoints = contours.flatMap { $0 }.map(\.p0)
let bottom = allPoints.map(\.y).max()!, top = allPoints.map(\.y).min()!
let baseline = dominantFlat(contours, near: bottom, window: (bottom - top) * 0.06)
let capTop = dominantFlat(contours, near: top, window: (bottom - top) * 0.06)
let unitsPerPixel = 700 / (baseline - capTop)
let left = allPoints.map(\.x).min()!

func snap(_ y: Double) -> Double {
	for flat in [baseline, capTop] where abs(y - flat) < 0.9 { return flat }
	return y
}

func units(_ p: Point) -> String {
	let x = (p.x - left) * unitsPerPixel
	let y = (snap(p.y) - baseline) * unitsPerPixel
	return "\(format(x)) \(format(y))"
}

func format(_ v: Double) -> String {
	let r = (v * 10).rounded() / 10
	return r == r.rounded() ? String(Int(r)) : String(format: "%.1f", r)
}

var d = ""
var nodes = 0
for cubics in contours {
	d += "M\(units(cubics[0].p0))"
	for c in cubics {
		d += c.isStraight ? "L\(units(c.p3))" : "C\(units(c.p1)) \(units(c.p2)) \(units(c.p3))"
		nodes += 1
	}
	d += "Z"
}

let maxX = (allPoints.map(\.x).max()! - left) * unitsPerPixel
let minY = (top - baseline) * unitsPerPixel
let maxY = (bottom - baseline) * unitsPerPixel
let svg = """
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 \(format(minY)) \(format(maxX)) \(format(maxY - minY))" data-source="\(source.lastPathComponent)" data-left="\(format(Double(cropX) + left / upscale))" data-baseline="\(format(Double(cropY) + baseline / upscale))" data-cap="\(format((baseline - capTop) / upscale))">
<path d="\(d)"/>
</svg>

"""
let out = root.appending(path: "design/brand/source/wordmark-traced.svg")
try FileManager.default.createDirectory(at: out.deletingLastPathComponent(), withIntermediateDirectories: true)
try svg.write(to: out, atomically: true, encoding: .utf8)
print("\(source.lastPathComponent): \(contours.count) contours, \(nodes) segments, cap height \(format(baseline - capTop)) px at \(Int(upscale))×")

// MARK: - Overlay

if let flag = CommandLine.arguments.firstIndex(of: "--overlay"), CommandLine.arguments.count > flag + 1 {
	let space = CGColorSpace(name: CGColorSpace.sRGB)!
	let ctx = CGContext(data: nil, width: upW, height: upH, bitsPerComponent: 8, bytesPerRow: 0, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
	ctx.translateBy(x: 0, y: CGFloat(upH))
	ctx.scaleBy(x: 1, y: -1)
	ctx.saveGState()
	ctx.translateBy(x: 0, y: CGFloat(upH))
	ctx.scaleBy(x: 1, y: -1)
	ctx.draw(smoothUp, in: CGRect(x: 0, y: 0, width: upW, height: upH))
	ctx.restoreGState()
	let path = CGMutablePath()
	for cubics in contours {
		path.move(to: CGPoint(x: cubics[0].p0.x, y: cubics[0].p0.y))
		for c in cubics { path.addCurve(to: CGPoint(x: c.p3.x, y: c.p3.y), control1: CGPoint(x: c.p1.x, y: c.p1.y), control2: CGPoint(x: c.p2.x, y: c.p2.y)) }
		path.closeSubpath()
	}
	ctx.addPath(path)
	ctx.setFillColor(CGColor(red: 1, green: 0.2, blue: 0.1, alpha: 0.35))
	ctx.fillPath()
	ctx.addPath(path)
	ctx.setStrokeColor(CGColor(red: 1, green: 0, blue: 0, alpha: 1))
	ctx.setLineWidth(1.5)
	ctx.strokePath()
	for cubics in contours { for c in cubics {
		ctx.setFillColor(CGColor(red: 0, green: 0.6, blue: 1, alpha: 1))
		ctx.fill(CGRect(x: c.p0.x - 3, y: c.p0.y - 3, width: 6, height: 6))
	} }
	let image = ctx.makeImage()!
	let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[flag + 1]) as CFURL, "public.png" as CFString, 1, nil)!
	CGImageDestinationAddImage(dest, image, nil)
	CGImageDestinationFinalize(dest)
}
