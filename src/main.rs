#![no_std]
#![no_main]

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct EfiGuid {
    pub data0: u32,
    pub data1: u16,
    pub data2: u16,
    pub data3: [u8; 8],
}  

const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid { 
    data0: 0x9042a9de,
    data1: 0x23dc,
    data2: 0x4a38,
    data3: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocol<'a> {
    reserved: [u64; 3],
    pub mode: &'a EfiGraphicsOutputProtocolMode<'a>,
}

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocolMode<'a> {
    pub max_mode: u32,
    pub mode: u32,
    pub info: &'a EfiGraphicsOutputProtocolModeInfo,
    pub size_of_info: usize,
    pub frame_buffer_base: u64,
    pub frame_buffer_size: usize,
}

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocolModeInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    _padding0: [u32; 5],
    pub pixels_per_scan_line: u32,
}

const _: () = assert!(size_of::<EfiGraphicsOutputProtocolModeInfo>() == 36);

fn locate_graphic_protocol<'a>(efi_system_table: &'a EfiSystemTable) -> Result<&'a EfiGraphicsOutputProtocol<'a>> {
    let mut graphics_output_protocol = null_mut::<EfiGraphicsOutputProtocol>(); 
    let status = (efi_system_table.boot_services.locate_protocol)(&EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID, null_mut::<EfiVoid>(), &mut graphics_output_protocol as *mut *mut EfiGraphicsOutputProtocol as *mut *mut EfiVoid);
    if status != EfiStatus::Success {
        return Err("Failed to locate graphics output protocol");
    }
    Ok(unsafe { &*graphics_output_protocol })
}

#[repr(C)]
struct EfiBootServicesTable{
   _reserved0: [u64; 40] ,
   locate_protocol: extern "win64" fn(protcol: *const EfiGuid, registration: *const EfiVoid, interface: *mut *mut EfiVoid) -> EfiStatus,
}

// コンパイルアサーションとして使うテクニック
const _: () = assert!(offset_of!(EfiBootServicesTable, locate_protocol) == 320);

#[no_mangle]
fn efi_main(_image_handle: EfiHandle, _efi_system_table: &EfiSystemTable) {
    let efi_graphics_output_protocol = locate_graphic_protocol(_efi_system_table).unwrap();
    let vram_addr = efi_graphics_output_protocol.mode.frame_buffer_base;
    let vram_byte_size = efi_graphics_output_protocol.mode.frame_buffer_size;
    let vram = unsafe {
        slice::from_raw_parts(vram_addr as *mut u32, vram_byte_size / size_of::<u32>());
    };
    
    for e in vram {
        *e = 0xffffff;
    }
    
    loop {}
}

use::core::panic::PanicInfo;
use core::{intrinsics::offset, mem::offset_of, slice};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
