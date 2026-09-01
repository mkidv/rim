#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub mod alpine_layout;
pub mod demo_layout;
pub mod uefi_layout;

#[cfg(test)]
mod tests;

use alpine_layout::{ALPINE_ROOTFS_SIZE_SECTORS, ALPINE_TOTAL_SIZE_BYTES, make_alpine_layout};
use demo_layout::{DEMO_IMAGE_SIZE_BYTES, make_demo_layout};
use rimgen::builder::build_on_io_with_events;
use rimio::{MemRimIO, SliceRimIO};
use uefi_layout::{UEFI_ESP_SIZE_SECTORS, UEFI_TOTAL_SIZE_BYTES, make_uefi_layout};

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[cfg(all(target_arch = "wasm32", not(test), not(feature = "host")))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

struct WasmState {
    image: Vec<u8>,
    report: Vec<u8>,
    error: Vec<u8>,
}

static STATE: core::sync::atomic::AtomicPtr<WasmState> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

fn get_state() -> &'static mut WasmState {
    let ptr = STATE.load(core::sync::atomic::Ordering::Acquire);
    if ptr.is_null() {
        let new_state = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(WasmState {
            image: Vec::new(),
            report: Vec::new(),
            error: Vec::new(),
        }));
        STATE.store(new_state, core::sync::atomic::Ordering::Release);
        unsafe { &mut *new_state }
    } else {
        unsafe { &mut *ptr }
    }
}

/// Performs the complete in-memory synthesis and internal validation.
pub fn synthesize_demo_image() -> Result<(Vec<u8>, String), String> {
    let mut buffer = alloc::vec![0u8; DEMO_IMAGE_SIZE_BYTES];
    let mut io = MemRimIO::new(&mut buffer);

    let (mut layout, _) = make_demo_layout();

    let mut event_count = 0usize;
    let report = build_on_io_with_events(&mut layout, &mut io, |_event| {
        event_count += 1;
    })
    .map_err(|e| format!("build_on_io failed: {e:?}"))?;

    // Internal disk validation verifies GPT header, partition array, CRC32, and backup tables.
    rimpart::validate_full_disk(&mut io).map_err(|e| format!("GPT validation failed: {e:?}"))?;

    let (_hdr, entries) = rimpart::gpt::read_gpt_with_sector(&mut io, 512)
        .map_err(|e| format!("Reading GPT entries failed: {e:?}"))?;

    let json_report = format!(
        "{{\"status\":\"ok\",\"total_bytes\":{},\"total_sectors\":{},\"partitions_count\":{},\"partitions\":[{{\"name\":\"{}\",\"fs\":\"FAT32\",\"size_bytes\":33554432}},{{\"name\":\"{}\",\"fs\":\"EXT4\",\"size_bytes\":25165824}}],\"events_emitted\":{}}}",
        report.total_bytes,
        report.total_sectors,
        entries.len(),
        report
            .partitions
            .first()
            .map(|p| p.name.as_str())
            .unwrap_or("ESP"),
        report
            .partitions
            .get(1)
            .map(|p| p.name.as_str())
            .unwrap_or("rootfs"),
        event_count,
    );

    Ok((buffer, json_report))
}

/// Performs the complete in-memory synthesis of the bootable Alpine Linux disk image.
pub fn synthesize_alpine_image(tar_bytes: &[u8]) -> Result<(Vec<u8>, String), String> {
    let mut buffer = alloc::vec![0u8; ALPINE_TOTAL_SIZE_BYTES];
    let mut io = MemRimIO::new(&mut buffer);

    let (mut layout, _) = make_alpine_layout(tar_bytes)?;

    let mut event_count = 0usize;
    let report = build_on_io_with_events(&mut layout, &mut io, |_event| {
        event_count += 1;
    })
    .map_err(|e| format!("build_on_io failed: {e:?}"))?;

    // Internal disk validation (verifies GPT header, partition array, CRC32, and backup tables)
    rimpart::validate_full_disk(&mut io).map_err(|e| format!("GPT validation failed: {e:?}"))?;

    let (_hdr, entries) = rimpart::gpt::read_gpt_with_sector(&mut io, 512)
        .map_err(|e| format!("Reading GPT entries failed: {e:?}"))?;

    let json_report = format!(
        "{{\"status\":\"ok\",\"kind\":\"alpine\",\"total_bytes\":{},\"total_sectors\":{},\"partitions_count\":{},\"partitions\":[{{\"name\":\"{}\",\"fs\":\"FAT32\",\"size_bytes\":67108864}},{{\"name\":\"{}\",\"fs\":\"EXT4\",\"size_bytes\":{}}}],\"events_emitted\":{}}}",
        report.total_bytes,
        report.total_sectors,
        entries.len(),
        report
            .partitions
            .first()
            .map(|p| p.name.as_str())
            .unwrap_or("ESP"),
        report
            .partitions
            .get(1)
            .map(|p| p.name.as_str())
            .unwrap_or("rootfs"),
        ALPINE_ROOTFS_SIZE_SECTORS * 512,
        event_count,
    );

    Ok((buffer, json_report))
}

/// Performs the in-memory synthesis of the bare-metal RIM.efi boot disk image.
pub fn synthesize_uefi_image(tar_bytes: &[u8]) -> Result<(Vec<u8>, String), String> {
    let mut buffer = alloc::vec![0u8; UEFI_TOTAL_SIZE_BYTES];
    let mut io = MemRimIO::new(&mut buffer);

    let (mut layout, _) = make_uefi_layout(tar_bytes)?;

    let mut event_count = 0usize;
    let report = build_on_io_with_events(&mut layout, &mut io, |_event| {
        event_count += 1;
    })
    .map_err(|e| format!("build_on_io failed: {e:?}"))?;

    rimpart::validate_full_disk(&mut io).map_err(|e| format!("GPT validation failed: {e:?}"))?;

    let json_report = format!(
        "{{\"status\":\"ok\",\"kind\":\"uefi\",\"total_bytes\":{},\"total_sectors\":{},\"partitions_count\":1,\"partitions\":[{{\"name\":\"ESP\",\"fs\":\"FAT32\",\"size_bytes\":{}}}],\"events_emitted\":{}}}",
        report.total_bytes,
        report.total_sectors,
        UEFI_ESP_SIZE_SECTORS * 512,
        event_count,
    );

    Ok((buffer, json_report))
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Inspects a disk image or container and returns a compact JSON partition report.
pub fn inspect_disk_image(image_bytes: &[u8]) -> Result<String, String> {
    let mut io = SliceRimIO::new(image_bytes);
    let format = rimimg::ImageFormat::from_read(&mut io)
        .map_err(|e| format!("image format detection failed: {e:?}"))?;
    let mut disk = rimimg::open_image_read_io(&mut io)
        .map_err(|e| format!("image container open failed: {e:?}"))?;
    let logical_bytes = disk.raw_len();
    let info = rimpart::scan_disk_with_sector(&mut disk, 512)
        .map_err(|e| format!("disk scan failed: {e:?}"))?;

    let mut out = format!(
        "{{\"status\":\"ok\",\"kind\":\"inspect\",\"image_format\":\"{}\",\"total_bytes\":{},\"logical_bytes\":{},\"sector_size\":{},\"mbr_kind\":\"{:?}\",\"gpt_present\":{},\"partitions_count\":{},\"partitions\":[",
        format,
        image_bytes.len(),
        logical_bytes,
        info.sector_size,
        info.mbr_kind,
        info.gpt_header.is_some(),
        info.partitions.len(),
    );

    for (idx, part) in info.partitions.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("{\"index\":");
        out.push_str(&part.index.to_string());
        out.push_str(",\"name\":");
        push_json_string(&mut out, &part.name);
        out.push_str(",\"type\":");
        push_json_string(&mut out, &format!("{}", part.kind));
        out.push_str(",\"start_lba\":");
        out.push_str(&part.start_lba.to_string());
        out.push_str(",\"end_lba\":");
        out.push_str(&part.end_lba.to_string());
        out.push_str(",\"start_bytes\":");
        out.push_str(&part.start_bytes.to_string());
        out.push_str(",\"size_bytes\":");
        out.push_str(&part.size_bytes.to_string());
        out.push('}');
    }

    out.push_str("]}");
    Ok(out)
}

// -----------------------------------------------------------------------------
// C ABI / WebAssembly Exports
// -----------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn wasm_synth_version() -> u32 {
    1
}

/// Triggers fast demo image synthesis. Returns 0 on success, -1 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn wasm_build_demo_image() -> i32 {
    let state = get_state();
    state.error.clear();
    match synthesize_demo_image() {
        Ok((image, report)) => {
            state.image = image;
            state.report = report.into_bytes();
            0
        }
        Err(err) => {
            state.image.clear();
            state.report.clear();
            state.error = err.into_bytes();
            -1
        }
    }
}

/// Triggers bootable Alpine Linux disk synthesis from payload TAR bytes.
///
/// # Safety
///
/// `tar_ptr` must point to a valid memory region of at least `tar_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_build_alpine_from_tar(tar_ptr: *const u8, tar_len: usize) -> i32 {
    if tar_ptr.is_null() || tar_len == 0 {
        let state = get_state();
        state.error = "Invalid TAR payload pointer or length".as_bytes().to_vec();
        return -1;
    }

    let tar_slice = unsafe { core::slice::from_raw_parts(tar_ptr, tar_len) };
    let state = get_state();
    state.error.clear();

    match synthesize_alpine_image(tar_slice) {
        Ok((image, report)) => {
            state.image = image;
            state.report = report.into_bytes();
            0
        }
        Err(err) => {
            state.image.clear();
            state.report.clear();
            state.error = err.into_bytes();
            -1
        }
    }
}

/// Triggers bare-metal RIM.efi boot disk synthesis from payload TAR bytes.
///
/// # Safety
///
/// `tar_ptr` must point to a valid memory region of at least `tar_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_build_uefi_from_tar(tar_ptr: *const u8, tar_len: usize) -> i32 {
    if tar_ptr.is_null() || tar_len == 0 {
        let state = get_state();
        state.error = "Invalid TAR payload pointer or length".as_bytes().to_vec();
        return -1;
    }

    let tar_slice = unsafe { core::slice::from_raw_parts(tar_ptr, tar_len) };
    let state = get_state();
    state.error.clear();

    match synthesize_uefi_image(tar_slice) {
        Ok((image, report)) => {
            state.image = image;
            state.report = report.into_bytes();
            0
        }
        Err(err) => {
            state.image.clear();
            state.report.clear();
            state.error = err.into_bytes();
            -1
        }
    }
}

/// Inspects a raw disk image from WASM memory. Returns 0 on success, -1 on failure.
///
/// # Safety
///
/// `image_ptr` must point to a valid memory region of at least `image_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_inspect_image(image_ptr: *const u8, image_len: usize) -> i32 {
    if image_ptr.is_null() || image_len == 0 {
        let state = get_state();
        state.error = "Invalid image pointer or length".as_bytes().to_vec();
        return -1;
    }

    let image_slice = unsafe { core::slice::from_raw_parts(image_ptr, image_len) };
    let state = get_state();
    state.error.clear();

    match inspect_disk_image(image_slice) {
        Ok(report) => {
            state.report = report.into_bytes();
            0
        }
        Err(err) => {
            state.report.clear();
            state.error = err.into_bytes();
            -1
        }
    }
}

/// Returns a pointer to the generated image buffer in WASM linear memory.
#[unsafe(no_mangle)]
pub extern "C" fn wasm_get_image_ptr() -> *const u8 {
    let state = get_state();
    state.image.as_ptr()
}

/// Returns the size in bytes of the generated image buffer.
#[unsafe(no_mangle)]
pub extern "C" fn wasm_get_image_len() -> usize {
    let state = get_state();
    state.image.len()
}

/// Returns a pointer to the JSON summary report in WASM linear memory.
#[unsafe(no_mangle)]
pub extern "C" fn wasm_get_report_ptr() -> *const u8 {
    let state = get_state();
    state.report.as_ptr()
}

/// Returns the length in bytes of the JSON summary report.
#[unsafe(no_mangle)]
pub extern "C" fn wasm_get_report_len() -> usize {
    let state = get_state();
    state.report.len()
}

/// Returns a pointer to the error message string (if build failed).
#[unsafe(no_mangle)]
pub extern "C" fn wasm_get_error_ptr() -> *const u8 {
    let state = get_state();
    state.error.as_ptr()
}

/// Returns the length of the error message string.
#[unsafe(no_mangle)]
pub extern "C" fn wasm_get_error_len() -> usize {
    let state = get_state();
    state.error.len()
}

/// Releases memory allocated for the image and report buffers.
#[unsafe(no_mangle)]
pub extern "C" fn wasm_free_buffers() {
    let state = get_state();
    state.image.clear();
    state.image.shrink_to_fit();
    state.report.clear();
    state.report.shrink_to_fit();
    state.error.clear();
    state.error.shrink_to_fit();
}
