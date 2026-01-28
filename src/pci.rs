use crate::acpi::AcpiMcfgDescriptor;
use crate::info;
use crate::result::Result;
use core::fmt::{self, write};
use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::Range;
use core::ptr::read_volatile;

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct VendorDeviceId {
    pub vendor: u16,
    pub device: u16,
}
impl VendorDeviceId {
    pub fn fmt_common(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(vendor: {:#06X}, device: {:#06X}",
            self.vendor, self.device
        )
    }
}
impl fmt::Debug for VendorDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_common(f)
    }
}
impl fmt::Display for VendorDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_common(f)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BusDeviceFunction {
    id: u16,
}
const MASK_BUS: usize = 0b1111_1111_0000_0000;
const SHIFT_BUS: usize = 8;
const MASK_DEVICE: usize = 0b0000_0000_1111_1000;
const SHIFT_DEVICE: usize = 3;
const MASK_FUNCTION: usize = 0b0000_0000_0000_0111;
const SHIFT_FUNCTION: usize = 0;
impl BusDeviceFunction {
    pub fn new(bus: usize, device: usize, function: usize) -> Result<Self> {
        if !(0..256).contains(&bus)
            || !(0..32).contains(&device)
            || !(0..8).contains(&function)
        {
            Err("PCI bus device function out of range")
        } else {
            Ok(Self {
                id: ((bus << SHIFT_BUS)
                    | (device << SHIFT_DEVICE)
                    | (function << SHIFT_FUNCTION)) as u16,
            })
        }
    }
    pub fn bus(&self) -> usize {
        ((self.id as usize) & MASK_BUS) >> SHIFT_BUS
    }
    pub fn device(&self) -> usize {
        ((self.id as usize) & MASK_DEVICE) >> SHIFT_DEVICE
    }
    pub fn function(&self) -> usize {
        ((self.id as usize) & MASK_FUNCTION) >> SHIFT_FUNCTION
    }
    pub fn iter() -> BusDeviceFunctionIterator {
        BusDeviceFunctionIterator { next_id: 0 }
    }
    pub fn fmt_common(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "/pci/bus/{:#04X}/device/{:#04X}/function/{:#03X}",
            self.bus(),
            self.device(),
            self.function()
        )
    }
}
impl fmt::Debug for BusDeviceFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_common(f)
    }
}
impl fmt::Display for BusDeviceFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_common(f)
    }
}
pub struct BusDeviceFunctionIterator {
    next_id: usize,
}
impl Iterator for BusDeviceFunctionIterator {
    type Item = BusDeviceFunction;
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next_id;
        if id > 0xffff {
            None
        } else {
            self.next_id += 1;
            let id = id as u16;
            Some(BusDeviceFunction { id })
        }
    }
}

struct ConfigureRegisters<T> {
    access_type: PhantomData<T>,
}
impl<T> ConfigureRegisters<T> {
    fn read(ecm_base: *mut T, byte_offset: usize) -> Result<T> {
        if !(0..256).contains(&byte_offset) || byte_offset % size_of::<T>() != 0
        {
            Err("PCI ConfigurationRegisters read out of range")
        } else {
            unsafe {
                Ok(read_volatile(ecm_base.add(byte_offset / size_of::<T>())))
            }
        }
    }
}
