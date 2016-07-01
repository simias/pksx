pub struct Interconnect {
    kernel: Box<[u8; KERNEL_SIZE]>,
    ram: Box<[u8; RAM_SIZE]>,
    /// When true the kernel is mirrored at address 0. Set on reset so
    /// that the reset vector starts executing from the kernel.
    kernel_at_0: bool,
}

impl Interconnect {
    pub fn new(kernel: Vec<u8>) -> Interconnect {
        assert!(kernel.len() == KERNEL_SIZE);

        let mut kernel_array = box_array![0; KERNEL_SIZE];

        for (a, &v) in kernel_array.iter_mut().zip(&kernel) {
            *a = v;
        }

        Interconnect {
            kernel: kernel_array,
            ram: box_array![0xca; RAM_SIZE],
            kernel_at_0: true,
        }
    }

    pub fn read<A: Addressable>(&self, addr: u32) -> u32 {

        if (addr & (A::size() as u32 - 1)) != 0 {
            panic!("Missaligned {}bit read at 0x{:08x}", A::size() * 8, addr);
        }

        let region = addr >> 24;
        let offset = addr & 0xffffff;

        match region {
            0x00 =>
                if self.kernel_at_0 {
                    self.read_kernel::<A>(offset)
                } else {
                    panic!("Ram access");
                },
            0x04 => self.read_kernel::<A>(offset),
            0x0a =>
                match offset {
                    // INT_LATCH
                    0 => 0,
                    // INT_MASK
                    8 => 0,
                    _ => panic!("Unsupported register 0x{:8x}", addr),
                },
            _ => panic!("unhandled load address 0x{:08x}", addr),
        }
    }

    pub fn store<A: Addressable>(&mut self, addr: u32, val: u32) {
        let region = addr >> 24;
        let offset = addr & 0xffffff;

        if (addr & (A::size() as u32 - 1)) != 0 {
            panic!("Missaligned {}bit store at 0x{:08x}",
                   A::size() * 8, addr);
        }

        match region {
            0x00 =>
                if !self.kernel_at_0 {
                    self.store_ram::<A>(offset, val);
                },
            0x06 =>
                match offset {
                    // F_CTRL
                    0 => self.set_f_ctrl::<A>(val),
                    _ => panic!("unhandled store address 0x{:08x}", addr),
                },
            _ => panic!("unhandled store address 0x{:08x}", addr),
        }
    }

    fn read_kernel<A: Addressable>(&self, offset: u32) -> u32 {
        let offset = offset as usize;

        let mut r = 0;

        for i in 0..A::size() as usize {
            r |= (self.kernel[offset + i] as u32) << (8 * i)
        }

        r
    }

    fn store_ram<A: Addressable>(&mut self, offset: u32, val: u32) {
        let offset = offset as usize;

        for i in 0..A::size() as usize {
            self.ram[offset + i] = (val >> (i * 8)) as u8;
        }
    }

    fn set_f_ctrl<A: Addressable>(&mut self, val: u32) {
        if A::size() != 1 {
            panic!("Unimplemented F_CTRL access");
        }

        match val {
            0x03 => self.kernel_at_0 = false,
            _ => panic!("unhandled F_CTRL 0x{:02x}", val),
        }
    }
}

/// Trait representing the attributes of a memory access
pub trait Addressable {
    /// Retreive the size of the access in bytes
    fn size() -> u8;
}

/// Marker for Byte (8bit) access
pub struct Byte;

impl Addressable for Byte {
    fn size() -> u8 {
        1
    }
}

/// Marker for Word (32bit) access
pub struct Word;

impl Addressable for Word {
    fn size() -> u8 {
        4
    }
}

/// Kernel size in bytes
const KERNEL_SIZE: usize = 16 * 1024;

/// RAM size in bytes
const RAM_SIZE: usize = 2 * 1024;
