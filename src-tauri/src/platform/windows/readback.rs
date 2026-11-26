use crate::platform::{desktop::ScreenRect, imaging::readback_origin};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};

/// Submit the ROI copy while the caller still owns the WGC/duplication frame.
/// Mapping the returned texture later synchronizes that copy, without reading
/// back an entire monitor for a subtitle or cloning a fullscreen image to crop it.
pub(super) fn stage_region(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    source: ScreenRect,
    content: (u32, u32),
    region: ScreenRect,
) -> Result<ID3D11Texture2D, String> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    let (left, top) = readback_origin(source, region, content, (desc.Width, desc.Height))?;
    let source_box = D3D11_BOX {
        left,
        top,
        front: 0,
        right: left + region.width,
        bottom: top + region.height,
        back: 1,
    };
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: region.width,
        Height: region.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: desc.Format,
        SampleDesc: desc.SampleDesc,
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..Default::default()
    };
    let mut staging = None;
    unsafe {
        device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
            .map_err(|e| format!("创建截图暂存纹理失败：{e}"))?;
        let staging = staging.ok_or("截图暂存纹理为空")?;
        context.CopySubresourceRegion(&staging, 0, 0, 0, 0, texture, 0, Some(&source_box));
        Ok(staging)
    }
}
