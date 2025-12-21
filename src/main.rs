#![no_std]
#![no_main]

use core::fmt::Write;
use core::panic::PanicInfo;
use core::writeln;
use wasabi::graphics::{draw_test_pattern, fill_rect, Bitmap};
use wasabi::init::init_basic_runtime;
use wasabi::qemu::exit_qemu;
use wasabi::qemu::QemuExitCode;
use wasabi::uefi::locate_loaded_image_protocol;
use wasabi::uefi::{
    init_vram, EfiHandle, EfiMemoryType, EfiSystemTable, VramTextWriter,
};
use wasabi::x86::{hlt, init_exceptions, trigger_debug_interrupt};
use wasabi::{info, println};

#[no_mangle]
fn efi_main(image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    println!("Booting Wasabi OS...");
    println!("image_handle: {:#18X}", image_handle);
    println!("efi_system_table: {:#p}", efi_system_table);

    let loaded_image_protocol =
        locate_loaded_image_protocol(image_handle, efi_system_table)
            .expect("Failed to locate Loaded Image Protocol");

    println!("image_base: {:#018X}", loaded_image_protocol.image_base);
    println!("image_size: {:#018X}", loaded_image_protocol.image_size);

    let mut vram =
        init_vram(efi_system_table).expect("Failed to initialize VRAM");
    let vw = vram.width();
    let vh = vram.height();

    fill_rect(&mut vram, 0x000000, 0, 0, vw, vh).expect("fill_rect failed.");
    draw_test_pattern(&mut vram);
    let mut w = VramTextWriter::new(&mut vram);

    let memory_map = init_basic_runtime(image_handle, efi_system_table);
    let mut total_memory_pages = 0;
    for e in memory_map.iter() {
        if e.memory_type() != EfiMemoryType::CONVENTIONAL_MEMORY {
            continue;
        }
        total_memory_pages += e.number_of_pages();
        writeln!(w, "{e:?}").unwrap();
    }
    let total_memory_size_mib = total_memory_pages * 4096 / 1024 / 1024;
    writeln!(
        w,
        "Total: {total_memory_pages} pages = {total_memory_size_mib} MiB"
    )
    .unwrap();

    writeln!(w, "Hello, Non-EFI World!").unwrap();

    let cr3 = wasabi::x86::read_cr3();
    println!("cr3 = {cr3:#p}");

    let t = Some(unsafe { &*cr3 });
    println!("{t:?}");
    let t = t.and_then(|t| t.next_level(0));
    println!("{t:?}");
    let t = t.and_then(|t| t.next_level(0));
    println!("{t:?}");
    let t = t.and_then(|t| t.next_level(0));
    println!("{t:?}");

    let (_gdt, _idt) = init_exceptions();
    info!("Exception initialized.");
    trigger_debug_interrupt();

    loop {
        hlt()
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit_qemu(QemuExitCode::Failed)
}
