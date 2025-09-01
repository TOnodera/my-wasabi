#![no_std]
#![no_main]

#[no_mangle]
fn efi_main(_image_handle: EfiHandle, _efi_system_table: &EfiSystemTable) {
    let efi_graphics_output_protocol = locate_graphic_protocol(_efi_system_table).unwrap();
    let vram_addr = efi_graphics_output_protocol.mode.frame_buffer_base;
    let vram_byte_size = efi_graphics_output_protocol.mode.frame_buffer_size;
    let vram = unsafe {
        slice::from_raw_parts(vram_addr as *mut u32, vram_byte_size / size_of::<u32>(), len)
    };
    
    for e in vram {
        *e = 0xffffff;
    }
    
    loop {}
}

use::core::panic::PanicInfo;
use core::slice;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
