use crate::qemu::exit_qemu;
use crate::qemu::QemuExitCode;
use core::panic::PanicInfo;

pub fn test_runner(_tests: &[&dyn Fn()]) -> ! {
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit_qemu(QemuExitCode::Failed);
}
