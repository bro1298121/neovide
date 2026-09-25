#![cfg(target_os = "windows")]

use std::time::{Duration, Instant};

use windows::{
    core::{Interface, Result},
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::{
                D3D11CreateDevice, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, ID3D11Device,
                ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
                D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_USAGE_STAGING,
            },
            Dxgi::{
                CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
                IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
            },
            Dxgi::Common::DXGI_SAMPLE_DESC,
        },
    },
};

const CAPTURE_INTERVAL: Duration = Duration::from_millis(150);
const MAX_CAPTURE_WIDTH: u32 = 512;

#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub r#gen: u64,
    pub data: Vec<u8>,
}

pub struct DesktopCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    factory: IDXGIFactory1,
    duplication: Option<IDXGIOutputDuplication>,
    staging: Option<ID3D11Texture2D>,
    output_rect: RECT,
    frame: Option<Frame>,
    r#gen: u64,
    last_area: Option<[i32; 4]>,
    last_tick: Instant,
}

impl DesktopCapture {
    pub fn new() -> Option<Self> {
        let factory = match unsafe { CreateDXGIFactory1::<IDXGIFactory1>() } {
            Ok(f) => f,
            Err(e) => {
                log::warn!("[desktop-capture] CreateDXGIFactory1 failed: {e:?}");
                return None;
            }
        };

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        if let Err(e) = unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        } {
            log::warn!("[desktop-capture] D3D11CreateDevice failed: {e:?}");
            return None;
        }
        let context = context?;
        let device = device?;

        log::info!("[desktop-capture] D3D11 device created");

        Some(DesktopCapture {
            device,
            context,
            factory,
            duplication: None,
            staging: None,
            output_rect: RECT::default(),
            frame: None,
            r#gen: 0,
            last_area: None,
            last_tick: Instant::now() - CAPTURE_INTERVAL,
        })
    }

    pub fn frame(&self) -> Option<&Frame> {
        self.frame.as_ref()
    }

    fn find_output(&self, center_x: i32, center_y: i32) -> Result<(IDXGIOutput1, RECT)> {
        let mut adapter_index = 0;
        loop {
            let adapter = match unsafe { self.factory.EnumAdapters(adapter_index) } {
                Ok(a) => a,
                Err(e) => return Err(e),
            };
            let mut output_index = 0;
            loop {
                let output = match unsafe { adapter.EnumOutputs(output_index) } {
                    Ok(o) => o,
                    Err(_) => break,
                };
                let desc = unsafe { output.GetDesc() }?;
                let rect = desc.DesktopCoordinates;
                if rect.left <= center_x
                    && center_x < rect.right
                    && rect.top <= center_y
                    && center_y < rect.bottom
                {
                    let output1: IDXGIOutput1 = output.cast()?;
                    return Ok((output1, rect));
                }
                output_index += 1;
            }
            adapter_index += 1;
        }
    }

    fn rebuild_duplication(&mut self, center_x: i32, center_y: i32) -> Result<()> {
        self.duplication.take();
        let (output, rect) = self.find_output(center_x, center_y)?;
        self.output_rect = rect;
        let dup = unsafe { output.DuplicateOutput(&self.device) }?;
        log::info!(
            "[desktop-capture] duplicated output left={} top={} right={} bottom={}",
            self.output_rect.left,
            self.output_rect.top,
            self.output_rect.right,
            self.output_rect.bottom
        );
        self.duplication = Some(dup);
        Ok(())
    }

    fn ensure_staging(&mut self, texture: &ID3D11Texture2D) -> Result<()> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        let staging_ok = self
            .staging
            .as_ref()
            .map(|s| {
                let mut d = D3D11_TEXTURE2D_DESC::default();
                unsafe { s.GetDesc(&mut d) };
                d.Width == desc.Width
                    && d.Height == desc.Height
                    && d.Format == desc.Format
                    && d.Usage == D3D11_USAGE_STAGING
                    && d.CPUAccessFlags == D3D11_CPU_ACCESS_READ.0 as u32
            })
            .unwrap_or(false);

        if staging_ok {
            return Ok(());
        }

        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: desc.Width,
            Height: desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
        }
        self.staging = staging;
        Ok(())
    }

    pub fn tick(&mut self, area: Option<[i32; 4]>) -> bool {
        let Some([left, top, right, bottom]) = area else {
            return false;
        };
        if right <= left || bottom <= top {
            return false;
        }
        let width = (right - left) as u32;
        let height = (bottom - top) as u32;

        if area == self.last_area && self.last_tick.elapsed() < CAPTURE_INTERVAL {
            return false;
        }
        self.last_tick = Instant::now();

        let center_x = (left + right) / 2;
        let center_y = (top + bottom) / 2;

        let covers_center = self
            .output_rect
            .left <= center_x
            && center_x < self.output_rect.right
            && self.output_rect.top <= center_y
            && center_y < self.output_rect.bottom;
        if self.duplication.is_none() || !covers_center {
            if let Err(e) = self.rebuild_duplication(center_x, center_y) {
                log::warn!("[desktop-capture] rebuild_duplication failed: {e:?}");
                return false;
            }
        }

        let Some(dup) = self.duplication.as_ref().cloned() else {
            return false;
        };

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;
        match unsafe { dup.AcquireNextFrame(0, &mut frame_info, &mut desktop_resource) } {
            Ok(_) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                if self.frame.is_none() {
                    log::debug!("[desktop-capture] no desktop frame yet (waiting for dirty region)");
                }
                return false;
            }
            Err(e) => {
                log::warn!("[desktop-capture] AcquireNextFrame failed: {e:?}");
                self.duplication.take();
                return false;
            }
        }

        let Some(resource) = desktop_resource else {
            let _ = unsafe { dup.ReleaseFrame() };
            return false;
        };

        let texture: ID3D11Texture2D = match resource.cast() {
            Ok(t) => t,
            Err(e) => {
                log::warn!("[desktop-capture] cast to texture failed: {e:?}");
                let _ = unsafe { dup.ReleaseFrame() };
                return false;
            }
        };

        if let Err(e) = self.ensure_staging(&texture) {
            log::warn!("[desktop-capture] ensure staging failed: {e:?}");
            let _ = unsafe { dup.ReleaseFrame() };
            return false;
        }

        let Some(staging) = self.staging.as_ref().cloned() else {
            let _ = unsafe { dup.ReleaseFrame() };
            return false;
        };

        let staging_resource: ID3D11Resource = match staging.cast() {
            Ok(r) => r,
            Err(_) => {
                let _ = unsafe { dup.ReleaseFrame() };
                return false;
            }
        };
        let texture_resource: ID3D11Resource = match texture.cast() {
            Ok(r) => r,
            Err(_) => {
                let _ = unsafe { dup.ReleaseFrame() };
                return false;
            }
        };
        unsafe {
            self.context.CopyResource(&staging_resource, &texture_resource);
        }
        let _ = unsafe { dup.ReleaseFrame() };

        let mut texture_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { staging.GetDesc(&mut texture_desc) };
        let tex_w = texture_desc.Width;
        let tex_h = texture_desc.Height;

        let origin_x = (left - self.output_rect.left).clamp(0, tex_w as i32 - 1) as u32;
        let origin_y = (top - self.output_rect.top).clamp(0, tex_h as i32 - 1) as u32;
        let cap_w = width.min(tex_w.saturating_sub(origin_x));
        let cap_h = height.min(tex_h.saturating_sub(origin_y));
        if cap_w == 0 || cap_h == 0 {
            return false;
        }

        let scaled = width > MAX_CAPTURE_WIDTH;
        let out_w = if scaled { MAX_CAPTURE_WIDTH } else { width.min(cap_w) };
        let out_h = (cap_h as u64 * out_w as u64).div_ceil(cap_w as u64) as u32;

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        if unsafe {
            self.context.Map(&staging_resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).is_err()
        } {
            return false;
        }

        let mut out = vec![0u8; (out_w * out_h * 4) as usize];
        let src_base = mapped.pData as *const u8;
        let row_pitch = mapped.RowPitch as usize;
        for y in 0..out_h {
            let src_y = if scaled {
                (origin_y as u64 * out_h as u64 + y as u64 * cap_h as u64) / out_h as u64
            } else {
                (origin_y + y) as u64
            } as usize;
            for x in 0..out_w {
                let src_x = if scaled {
                    (origin_x as u64 * out_w as u64 + x as u64 * cap_w as u64) / out_w as u64
                } else {
                    (origin_x + x) as u64
                } as usize;
                let dst = ((y * out_w + x) * 4) as usize;
                unsafe {
                    let src = src_base.add(src_y * row_pitch + src_x * 4);
                    out[dst..dst + 4].copy_from_slice(std::slice::from_raw_parts(src, 4));
                }
            }
        }
        unsafe { self.context.Unmap(&staging_resource, 0) };

        self.r#gen += 1;
        if self.frame.is_none() {
            log::info!(
                "[desktop-capture] first frame captured: {}x{} from region {}x{} origin=({},{})",
                out_w,
                out_h,
                cap_w,
                cap_h,
                origin_x,
                origin_y
            );
        }
        self.frame = Some(Frame { width: out_w, height: out_h, r#gen: self.r#gen, data: out });
        self.last_area = area;
        true
    }
}