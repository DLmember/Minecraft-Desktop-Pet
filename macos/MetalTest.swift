import AppKit
import IOSurface
import Metal
import QuartzCore

/// Render the shared Rust geometry into a transparent desktop window.
final class MetalTestRenderer {
    let layer: CALayer
    var targetSize: CGSize? { depth.map { CGSize(width: $0.width, height: $0.height) } }
    var usesSurfaces: Bool { surfaceLayer != nil }
    private let renderPass = MTLRenderPassDescriptor()
    private var generation: UInt64 = 0
    private var failedSurface = false
    private let drawableLayer: CAMetalLayer?
    private let surfaceLayer: CALayer?
    private final class SurfaceSlot {
        let surface: IOSurfaceRef
        let texture: MTLTexture
        var busy = false
        var lastUpdate: Int?
        init(surface: IOSurfaceRef, texture: MTLTexture) {
            self.surface = surface
            self.texture = texture
        }
    }
    private var surfacePool: [SurfaceSlot] = []
    private let poolSurfaces: Bool
    let device: MTLDevice
    let queue: MTLCommandQueue
    let pipeline: MTLRenderPipelineState
    let depthState: MTLDepthStencilState
    let overlayDepthState: MTLDepthStencilState
    let shadeTable: MTLBuffer
    private var depth: MTLTexture?
    private var offscreenColor: MTLTexture?
    private let offscreen: Bool
    private let trackPresentations: Bool
    private var textures: [UInt: MTLTexture] = [:]
    private let available = DispatchSemaphore(value: 3)
    private var vertexBuffers: [MTLBuffer?] = [nil, nil, nil]
    private var slot = 0
    private let stats = NSLock()
    private var presented = 0
    private(set) var surfaceUpdates = 0
    var presentations: Int {
        stats.lock(); defer { stats.unlock() }
        return presented
    }

    init(offscreen: Bool = false, trackPresentations: Bool = true,
         useSurface: Bool = false, poolSurfaces: Bool = false) throws {
        self.offscreen = offscreen
        self.trackPresentations = trackPresentations
        self.poolSurfaces = poolSurfaces
        if useSurface {
            let surface = CALayer()
            surface.actions = ["contents": NSNull(), "hidden": NSNull(), "bounds": NSNull(), "position": NSNull()]
            self.layer = surface
            self.surfaceLayer = surface
            self.drawableLayer = nil
        } else {
            let drawable = CAMetalLayer()
            self.layer = drawable
            self.drawableLayer = drawable
            self.surfaceLayer = nil
        }
        guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else {
            throw PetError("Metal device unavailable")
        }
        self.device = device
        self.queue = queue
        let library = try device.makeLibrary(source: Self.shader, options: nil)
        let descriptor = MTLRenderPipelineDescriptor()
        descriptor.vertexFunction = library.makeFunction(name: "pet_vertex")
        descriptor.fragmentFunction = library.makeFunction(name: "pet_fragment")
        descriptor.depthAttachmentPixelFormat = .depth32Float
        let color = descriptor.colorAttachments[0]!
        color.pixelFormat = .bgra8Unorm
        color.isBlendingEnabled = true
        color.sourceRGBBlendFactor = .one
        color.destinationRGBBlendFactor = .oneMinusSourceAlpha
        color.sourceAlphaBlendFactor = .one
        color.destinationAlphaBlendFactor = .oneMinusSourceAlpha
        pipeline = try device.makeRenderPipelineState(descriptor: descriptor)
        let test = MTLDepthStencilDescriptor()
        test.depthCompareFunction = .less
        test.isDepthWriteEnabled = true
        let overlayTest = MTLDepthStencilDescriptor()
        overlayTest.depthCompareFunction = .always
        guard let state = device.makeDepthStencilState(descriptor: test),
              let overlayState = device.makeDepthStencilState(descriptor: overlayTest),
              let shades = device.makeBuffer(bytes: pet_shade_table(), length: 6 * 3 * 256) else {
            throw PetError("Metal depth or shading resources unavailable")
        }
        depthState = state
        overlayDepthState = overlayState
        shadeTable = shades
        drawableLayer?.device = device
        drawableLayer?.pixelFormat = .bgra8Unorm
        drawableLayer?.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        drawableLayer?.framebufferOnly = true
        layer.isOpaque = false
        layer.magnificationFilter = .nearest
        layer.minificationFilter = .nearest
    }

    func invalidateTextures() {
        textures.removeAll(keepingCapacity: true)
        generation &+= 1
    }

    func hideForSoftware() {
        guard !layer.isHidden else { return }
        generation &+= 1
        layer.isHidden = true
        surfaceLayer?.contents = nil
    }

    private func texture(for batch: PetMetalBatch) -> MTLTexture? {
        guard let pixels = batch.pixels else { return nil }
        let key = UInt(bitPattern: pixels)
        if let cached = textures[key] { return cached }
        let w = Int(batch.width), h = Int(batch.height)
        let descriptor = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm,
                                                                   width: w, height: h, mipmapped: false)
        descriptor.usage = .shaderRead
        guard let texture = device.makeTexture(descriptor: descriptor) else { return nil }
        texture.replace(region: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0, withBytes: pixels, bytesPerRow: w * 4)
        textures[key] = texture
        return texture
    }

    private func surfaceSlot(width: Int, height: Int) -> SurfaceSlot? {
        if poolSurfaces {
            if let cached = surfacePool.first(where: { candidate in
                guard !candidate.busy else { return false }
                guard let last = candidate.lastUpdate else { return true }
                // Keep the latest two layer updates reserved during CA's asynchronous
                // handoff. Recycle older surfaces only when WindowServer releases them.
                return surfaceUpdates - last >= 2 && !IOSurfaceIsInUse(candidate.surface)
            }) { return cached }
            guard surfacePool.count < 6 else { return nil }
        }
        let properties: [String: Any] = [kIOSurfaceWidth as String: width,
                                          kIOSurfaceHeight as String: height,
                                          kIOSurfaceBytesPerElement as String: 4,
                                          kIOSurfacePixelFormat as String: UInt32(0x42475241)] // BGRA
        let descriptor = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm,
                                                                   width: width, height: height, mipmapped: false)
        descriptor.usage = .renderTarget
        descriptor.storageMode = .shared
        guard let surface = IOSurfaceCreate(properties as CFDictionary),
              let target = device.makeTexture(descriptor: descriptor, iosurface: surface, plane: 0) else { return nil }
        let slot = SurfaceSlot(surface: surface, texture: target)
        if poolSurfaces { surfacePool.append(slot) }
        return slot
    }

    @discardableResult
    func draw(_ mesh: PetMetalMesh, size: CGSize, snapshot: URL? = nil) -> Bool {
        guard !failedSurface else { return false }
        guard available.wait(timeout: .now()) == .success else { return false }
        var submitted = false
        defer { if !submitted { available.signal() } }
        let width = Int(size.width), height = Int(size.height)
        if depth?.width != width || depth?.height != height {
            drawableLayer?.drawableSize = size
            generation &+= 1
            surfacePool.removeAll(keepingCapacity: true)
            let descriptor = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .depth32Float,
                                                                       width: width, height: height, mipmapped: false)
            descriptor.usage = .renderTarget
            descriptor.storageMode = device.supportsFamily(.apple1) ? .memoryless : .private
            depth = device.makeTexture(descriptor: descriptor)
        }
        guard let depth, let command = queue.makeCommandBuffer() else { return false }
        var drawable: CAMetalDrawable?
        var newSurface: SurfaceSlot?
        let color: MTLTexture
        if snapshot == nil, !offscreen, let drawableLayer {
            guard let next = drawableLayer.nextDrawable() else { return false }
            drawable = next
            color = next.texture
        } else if snapshot == nil && surfaceLayer != nil {
            guard let surface = surfaceSlot(width: width, height: height) else { return false }
            newSurface = surface
            color = surface.texture
        } else {
            let descriptor = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm,
                                                                       width: width, height: height, mipmapped: false)
            descriptor.usage = .renderTarget
            descriptor.storageMode = snapshot == nil ? .private : .shared
            if snapshot != nil {
                guard let target = device.makeTexture(descriptor: descriptor) else { return false }
                color = target
            } else {
                if offscreenColor?.width != width || offscreenColor?.height != height {
                    offscreenColor = device.makeTexture(descriptor: descriptor)
                }
                guard let offscreenColor else { return false }
                color = offscreenColor
            }
        }
        let pass = renderPass
        pass.colorAttachments[0].texture = color
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].storeAction = .store
        pass.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 0, 0)
        pass.depthAttachment.texture = depth
        pass.depthAttachment.loadAction = .clear
        pass.depthAttachment.storeAction = .dontCare
        pass.depthAttachment.clearDepth = 1
        let encodedPass = command.makeRenderCommandEncoder(descriptor: pass)
        // Drop the target reference immediately, particularly for a CAMetalDrawable.
        pass.colorAttachments[0].texture = nil
        guard let encoder = encodedPass else { return false }
        defer { if !submitted { encoder.endEncoding() } }
        encoder.setRenderPipelineState(pipeline)
        encoder.setDepthStencilState(depthState)
        if mesh.vertex_count > 0, let vertices = mesh.vertices {
            let length = Int(mesh.vertex_count) * MemoryLayout<PetMetalVertex>.stride
            if vertexBuffers[slot]?.length ?? 0 < length {
                vertexBuffers[slot] = device.makeBuffer(length: max(length, 4096), options: .storageModeShared)
            }
            guard let buffer = vertexBuffers[slot] else { return false }
            memcpy(buffer.contents(), vertices, length)
            encoder.setVertexBuffer(buffer, offset: 0, index: 0)
            var dimensions = SIMD2<Float>(Float(width), Float(height))
            encoder.setVertexBytes(&dimensions, length: MemoryLayout.size(ofValue: dimensions), index: 1)
            encoder.setFragmentBuffer(shadeTable, offset: 0, index: 0)
            for i in 0..<Int(mesh.batch_count) {
                let batch = mesh.batches[i]
                guard let texture = texture(for: batch) else { return false }
                encoder.setDepthStencilState(batch.overlay ? overlayDepthState : depthState)
                encoder.setFragmentTexture(texture, index: 0)
                encoder.drawPrimitives(type: .triangle, vertexStart: Int(batch.start), vertexCount: Int(batch.count))
            }
        }
        encoder.endEncoding()
        submitted = true
        slot = (slot + 1) % vertexBuffers.count
        if let newSurface {
            let submittedGeneration = generation
            newSurface.busy = true
            command.addCompletedHandler { [weak self, available] command in
                let completed = command.status == .completed
                DispatchQueue.main.async {
                    defer { newSurface.busy = false; available.signal() }
                    guard let self, self.generation == submittedGeneration, let surfaceLayer = self.surfaceLayer else { return }
                    guard completed else { self.failedSurface = true; return }
                    surfaceLayer.contents = newSurface.surface
                    self.surfaceUpdates += 1
                    newSurface.lastUpdate = self.surfaceUpdates
                }
            }
        } else {
            command.addCompletedHandler { [available] _ in available.signal() }
        }
        if let drawable {
            if trackPresentations {
                drawable.addPresentedHandler { [weak self] drawable in
                    guard drawable.presentedTime > 0, let self else { return }
                    self.stats.lock(); self.presented += 1; self.stats.unlock()
                }
            }
            command.present(drawable)
        }
        command.commit()
        if let snapshot {
            command.waitUntilCompleted()
            guard command.status == .completed else { return false }
            return Self.writeSnapshot(color, to: snapshot)
        }
        return true
    }

    private static func writeSnapshot(_ texture: MTLTexture, to path: URL) -> Bool {
        let w = texture.width, h = texture.height
        var pixels = Data(count: w * h * 4)
        pixels.withUnsafeMutableBytes { raw in
            texture.getBytes(raw.baseAddress!, bytesPerRow: w * 4, from: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0)
        }
        do {
            try pixels.write(to: path)
            guard let provider = CGDataProvider(data: pixels as CFData),
                  let image = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32,
                                      bytesPerRow: w * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                      bitmapInfo: [.byteOrder32Little, CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue)],
                                      provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent),
                  let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else { return false }
            try png.write(to: path.deletingPathExtension().appendingPathExtension("png"))
            return true
        } catch { return false }
    }

    private static let shader = """
    #include <metal_stdlib>
    using namespace metal;

    struct Vertex { float x, y, z, u, v, baseU, baseV, width, depth; uint mode; };
    struct Fragment {
        float4 position [[position]];
        float2 uv;
        float4 keyboard [[flat]];
        uint mode [[flat]];
    };
    vertex Fragment pet_vertex(uint id [[vertex_id]], const device Vertex *vertices [[buffer(0)]],
                               constant float2 &size [[buffer(1)]]) {
        const Vertex v = vertices[id];
        const float2 p = float2(v.x, v.y) / size;
        return {float4(p.x * 2 - 1, 1 - p.y * 2, (v.z + 256.0) / 512.0, 1),
                float2(v.u, v.v), float4(v.baseU, v.baseV, v.width, v.depth), v.mode};
    }
    fragment float4 pet_fragment(Fragment f [[stage_in]], texture2d<float, access::read> image [[texture(0)]],
                                  constant uchar *shades [[buffer(0)]]) {
        if (f.mode & 0x80000000u) {
            float3 rgb = float3((f.mode >> 16) & 255, (f.mode >> 8) & 255, f.mode & 255);
            if (f.mode & 0x40000000u) {
                float2 local = f.uv - f.keyboard.xy - float2(f.keyboard.w, 0);
                float row = round((local.y / f.keyboard.w - 0.2) / 0.3);
                float stagger = row == 1 ? 0.025 : -0.025;
                float col = round(local.x / f.keyboard.z * 6 - stagger * 6);
                float2 center = float2((col / 6 + stagger) * f.keyboard.z, (0.2 + row * 0.3) * f.keyboard.w);
                if (row >= 0 && row <= 2 && col >= 1 && col <= 5
                    && all(abs(local - center) <= f.keyboard.w * 0.11)) rgb = round(rgb * 0.78);
            }
            return float4(rgb / 255.0, 1);
        }
        if (f.mode & 0x20000000u) {
            uint2 samplePoint = uint2(clamp(f.uv, 0.0, 23.0));
            samplePoint.y += uint(f.keyboard.y);
            float alpha = round(image.read(samplePoint).a * f.keyboard.x * 255.0) / 255.0;
            float3 rgb = float3((f.mode >> 16) & 255, (f.mode >> 8) & 255, f.mode & 255);
            return float4(rgb * (alpha / 255.0), alpha);
        }
        uint2 size = uint2(image.get_width(), image.get_height());
        uint2 coord = min(uint2(max(f.uv * (float(size.x) / 64.0), 0.0)), size - 1);
        float4 texel = image.read(coord);
        uint4 rgba = uint4(round(texel * 255.0));
        if (rgba.a < 16) discard_fragment();
        uint offset = f.mode * 768;
        float3 rgb = float3(shades[offset + rgba.r], shades[offset + 256 + rgba.g], shades[offset + 512 + rgba.b]);
        return float4(rgb * (texel.a / 255.0), texel.a);
    }
    """
}
