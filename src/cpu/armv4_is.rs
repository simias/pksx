//! ARMv4 instruction set

use std::fmt;

use memory::{Word, HalfWord, Byte};
use debugger::Debugger;

use super::{Cpu, RegisterIndex};

pub fn execute<D>(cpu: &mut Cpu, debugger: &mut D, instruction: u32)
    where D: Debugger {
    let instruction = Instruction(instruction);

    instruction.execute(debugger, cpu);
}

/// Wrapper around a 32bit instruction word
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Instruction(u32);

impl Instruction {
    /// We decode the instruction based on bits [27:20] and
    /// [7:4]. That gives us 4096 possibilities.
    fn opcode(self) -> u32 {
        let opcode_hi = (self.0 >> 20) & 0xff;
        let opcode_lo = (self.0 >> 4) & 0xf;

        (opcode_hi << 4) | opcode_lo
    }

    fn condition_code(self) -> u32 {
        self.0 >> 28
    }

    fn rn(self) -> RegisterIndex {
        RegisterIndex((self.0 >> 16) & 0xf)
    }

    fn rd(self) -> RegisterIndex {
        RegisterIndex((self.0 >> 12) & 0xf)
    }

    fn rs(self) -> RegisterIndex {
        RegisterIndex((self.0 >> 8) & 0xf)
    }

    fn rm(self) -> RegisterIndex {
        RegisterIndex(self.0 & 0xf)
    }

    fn branch_imm_offset(self) -> u32 {
        // offset must be sign-extented
        let offset = (self.0 << 8) as i32;

        // Offset must be multiplied by 4 so we only shift it down 6
        // places.
        (offset >> 6) as u32
    }

    fn msr_field_mask(self) -> u32 {
        (self.0 >> 16) & 0xf
    }

    /// Register list for load/store multiple.
    fn register_list(self) -> u32 {
        self.0 & 0xffff
    }

    /// Execute this instruction
    fn execute<D>(self, debugger: &mut D, cpu: &mut Cpu)
        where D: Debugger {

        let n = cpu.n();
        let z = cpu.z();
        let c = cpu.c();
        let v = cpu.v();

        // All ARM instructions have a 4bit "condition" code which can
        // be used to conditionally execute an instruction without
        // having to use a branch
        let cond_true =
            match self.condition_code() {
                // Equal (EQ)
                0b0000 => z,
                // Not equal (NE)
                0b0001 => !z,
                // Unsigned higher, or same (CS)
                0b0010 => c,
                // Unsigned lower (CC)
                0b0011 => !c,
                // Negative (MI)
                0b0100 => n,
                // Positive, or 0 (PL)
                0b0101 => !n,
                // Overflow (VS)
                0b0110 => v,
                // No overflow (VC)
                0b0111 => !v,
                // Unsigned higher (HI)
                0b1000 => c && !z,
                // Unsigned lower, or same (LS)
                0b1001 => !c || z,
                // Greater, or equal (GE)
                0b1010 => n == v,
                // Less than (LT)
                0b1011 => n != v,
                // Greater than (GT)
                0b1100 => !z && (n == v),
                // Less than, or equal (LE)
                0b1101 => z || (n != v),
                // Always (AL)
                0b1110 => true,
                // This condition code is "unpredictable".
                0b1111 => panic!("Unexpected ARM condition 0b1111"),
                _ => unreachable!(),
            };

        if cond_true {
            self.decode_and_execute(debugger, cpu);
        }
    }

    fn decode_and_execute<D>(self, debugger: &mut D, cpu: &mut Cpu)
        where D: Debugger {
        let handler = OPCODE_LUT[self.opcode() as usize];

        handler(self, debugger, cpu);
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{:08x}", self.0)
    }
}

/// Addressing mode 1: Data-processing operands
trait Mode1Addressing {
    /// Return the value of the operand
    fn value(instruction: Instruction, cpu: &Cpu) -> u32;

    /// Return the value of the operand along with the ALU carry
    fn value_carry(instruction: Instruction, cpu: &Cpu) -> (u32, bool);

    /// Used to validate that the addressing mode matches the
    /// instruction (useful for debugging).
    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool;
}

struct Mode1Imm;

impl Mode1Addressing for Mode1Imm {
    fn value(instruction: Instruction, _: &Cpu) -> u32 {
        let rot = (instruction.0 >> 8) & 0xf;
        let imm = instruction.0 & 0xff;

        // Rotation factor is multiplied by two
        imm.rotate_right(rot << 1)
    }

    fn value_carry(instruction: Instruction, cpu: &Cpu) -> (u32, bool) {
        let rot = (instruction.0 >> 8) & 0xf;
        let imm = instruction.0 & 0xff;

        if rot == 0 {
            (imm, cpu.c())
        } else {
            // Rotation factor is multiplied by two
            let imm = imm.rotate_right(rot << 1);

            let carry_out = (imm as i32) < 0;

            (imm, carry_out)
        }
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 20) & 1) == s as u32 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 25) & 7) == 1
    }
}

struct Mode1LslImm;

impl Mode1Addressing for Mode1LslImm {
    fn value(instruction: Instruction, cpu: &Cpu) -> u32 {
        let shift = (instruction.0 >> 7) & 0x1f;
        let rm    = instruction.rm();
        let val   = cpu.reg(rm);

        val << shift
    }

    fn value_carry(instruction: Instruction, cpu: &Cpu) -> (u32, bool) {
        let shift = (instruction.0 >> 7) & 0x1f;
        let rm    = instruction.rm();
        let val   = cpu.reg(rm);

        match shift {
            0 => (val, cpu.c()),
            _ => {
                let carry = ((val << (shift - 1)) & 0x80000000) != 0;

                (val << shift, carry)
            }
        }
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 20) & 1) == s as u32 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 25) & 7) == 0 &&
            ((i >> 4) & 7) == 0
    }
}

struct Mode1LsrImm;

impl Mode1Addressing for Mode1LsrImm {
    fn value(instruction: Instruction, cpu: &Cpu) -> u32 {
        let shift = (instruction.0 >> 7) & 0x1f;
        let rm    = instruction.rm();
        let val   = cpu.reg(rm);

        match shift {
            // Shift 0 means shift by 32
            0 => 0,
            _ => val >> shift
        }
    }

    fn value_carry(instruction: Instruction, cpu: &Cpu) -> (u32, bool) {
        let shift = (instruction.0 >> 7) & 0x1f;
        let rm    = instruction.rm();
        let val   = cpu.reg(rm);

        match shift {
            // Shift 0 means shift by 32
            0 => (0, (val as i32) < 0),
            _ => {
                let carry = (val >> (shift - 1)) & 1 != 0;

                (val >> shift, carry)
            }
        }
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 20) & 1) == s as u32 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 25) & 7) == 0 &&
            ((i >> 4) & 7) == 0b010
    }
}

struct Mode1LslReg;

impl Mode1Addressing for Mode1LslReg {
    fn value(instruction: Instruction, cpu: &Cpu) -> u32 {
        let rm    = instruction.rm();
        let rs    = instruction.rs();
        let val   = cpu.reg(rm);
        let shift = cpu.reg(rs) & 0xff;

        match shift {
            0...31 => val << shift,
            _ => 0,
        }
    }

    fn value_carry(_instruction: Instruction, _cpu: &Cpu) -> (u32, bool) {
        unimplemented!();
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 20) & 1) == s as u32 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 25) & 7) == 0 &&
            ((i >> 4) & 0xf) == 0b0001
    }
}

struct Mode1LsrReg;

impl Mode1Addressing for Mode1LsrReg {
    fn value(instruction: Instruction, cpu: &Cpu) -> u32 {
        let rm    = instruction.rm();
        let rs    = instruction.rs();
        let val   = cpu.reg(rm);
        let shift = cpu.reg(rs) & 0xff;

        match shift {
            0...31 => val >> shift,
            _ => 0,
        }
    }

    fn value_carry(_instruction: Instruction, _cpu: &Cpu) -> (u32, bool) {
        unimplemented!();
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 20) & 1) == s as u32 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 25) & 7) == 0 &&
            ((i >> 4) & 0xf) == 0b0011
    }
}

struct Mode1AsrImm;

impl Mode1Addressing for Mode1AsrImm {
    fn value(instruction: Instruction, cpu: &Cpu) -> u32 {
        let shift = (instruction.0 >> 7) & 0x1f;
        let rm    = instruction.rm();
        let val   = cpu.reg(rm) as i32;

        let val =
            match shift {
                // Shift 0 means shift by 32, which is like shifting
                // by 31 when using a signed value (i.e. the sign bit
                // is replicated all over the 32bits)
                0 => val >> 31,
                _ => val >> shift
            };

        val as u32
    }

    fn value_carry(instruction: Instruction, cpu: &Cpu) -> (u32, bool) {
        let shift = (instruction.0 >> 7) & 0x1f;
        let rm    = instruction.rm();
        let val   = cpu.reg(rm) as i32;

        let (val, c) =
            match shift {
                // Shift 0 means shift by 32, which is like shifting
                // by 31 when using a signed value (i.e. the sign bit
                // is replicated all over the 32bits)
                0 => (val >> 31, val < 0),
                _ => (val >> shift, (val >> (shift - 1)) & 1 != 0),
            };

        (val as u32, c)
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 20) & 1) == s as u32 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 25) & 7) == 0 &&
            ((i >> 4) & 7) == 0b100
    }
}

struct Mode1AsrReg;

impl Mode1Addressing for Mode1AsrReg {
    fn value(instruction: Instruction, cpu: &Cpu) -> u32 {
        let rm    = instruction.rm();
        let rs    = instruction.rs();
        let val   = cpu.reg(rm) as i32;
        let shift = cpu.reg(rs) & 0xff;

        let r =
            match shift {
                0...31 => val >> shift,
                // Shift by more than 31 is like shifting by 31 when
                // using a signed value (i.e. the sign bit is
                // replicated all over the 32bits)
                _ => val >> 31,
            };

        r as u32
    }

    fn value_carry(_instruction: Instruction, _cpu: &Cpu) -> (u32, bool) {
        unimplemented!();
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 20) & 1) == s as u32 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 25) & 7) == 0 &&
            ((i >> 4) & 0xf) == 0b0101
    }
}

/// Rotate right by immediate/ Rotate right with extend
struct Mode1RorImm;

impl Mode1Addressing for Mode1RorImm {
    fn value(instruction: Instruction, cpu: &Cpu) -> u32 {
        let shift = (instruction.0 >> 7) & 0x1f;
        let rm    = instruction.rm();

        let val = cpu.reg(rm);

        if shift == 0 {
            // Special case: this is rotate right with extend
            let c = cpu.c() as u32;

            (c << 31) | (val >> 1)
        } else {
            val.rotate_right(shift)
        }
    }

    fn value_carry(instruction: Instruction, cpu: &Cpu) -> (u32, bool) {
        let shift = (instruction.0 >> 7) & 0x1f;
        let rm    = instruction.rm();

        let val = cpu.reg(rm);

        if shift == 0 {
            // Special case: this is rotate right with extend
            let c = cpu.c() as u32;

            let r = (c << 31) | (val >> 1);

            (r, (r & 1) != 0)
        } else {
            (val.rotate_right(shift),
             ((val >> (shift - 1)) & 1) != 0)
        }
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 25) & 7) == 0 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 20) & 1) == s as u32 &&
            ((i >> 4) & 7) == 0b110
    }
}

struct Mode1RorReg;

impl Mode1Addressing for Mode1RorReg {
    fn value(instruction: Instruction, cpu: &Cpu) -> u32 {
        let rm    = instruction.rm();
        let rs    = instruction.rs();

        let val   = cpu.reg(rm);
        let shift = cpu.reg(rs) & 0x1f;

        val.rotate_right(shift)
    }

    fn value_carry(instruction: Instruction, cpu: &Cpu) -> (u32, bool) {
        let rm    = instruction.rm();
        let rs    = instruction.rs();

        let val   = cpu.reg(rm);
        let shift = cpu.reg(rs) & 0xff;

        if shift == 0 {
            (val, cpu.c())
        } else if (shift & 0x1f) == 0 {
            (val, (val as i32) < 0)
        } else {
            let shift = shift & 0x1f;

            (val.rotate_right(shift),
             ((val >> (shift - 1)) & 1) != 0)
        }
    }

    fn is_valid(instruction: Instruction, opcode: u32, s: bool) -> bool {
        let i = instruction.0;

        ((i >> 20) & 1) == s as u32 &&
            ((i >> 21) & 0xf) == opcode &&
            ((i >> 25) & 7) == 0 &&
            ((i >> 4) & 0xf) == 0b0111
    }
}

fn unimplemented(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu) {
    panic!("Unimplemented instruction {} ({:03x})\n{:?}",
           instruction,
           instruction.opcode(),
           cpu);
}

fn and<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd = instruction.rd();
    let rn = instruction.rn();
    let b  = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 0, false));

    let a = cpu.reg(rn);

    let val = a & b;

    cpu.set_reg(rd, val);
}

fn ands<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd     = instruction.rd();
    let rn     = instruction.rn();
    let (b, c) = M::value_carry(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 0, true));

    if rd.is_pc() {
        panic!("*S instruction with PC target");
    }

    let a = cpu.reg(rn);

    let val = a & b;

    cpu.set_reg(rd, val);

    cpu.set_n((val as i32) < 0);
    cpu.set_z(val == 0);
    cpu.set_c(c);
}

fn eor<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd = instruction.rd();
    let rn = instruction.rn();
    let b  = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 1, false));

    let a = cpu.reg(rn);

    let val = a ^ b;

    cpu.set_reg(rd, val);
}

fn eors<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd     = instruction.rd();
    let rn     = instruction.rn();
    let (b, c) = M::value_carry(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 1, true));

    if rd.is_pc() {
        panic!("*S instruction with PC target");
    }

    let a = cpu.reg(rn);

    let val = a ^ b;

    cpu.set_reg(rd, val);

    cpu.set_n((val as i32) < 0);
    cpu.set_z(val == 0);
    cpu.set_c(c);
}

fn sub<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let dst = instruction.rd();
    let rn  = instruction.rn();
    let b   = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 2, false));

    let a = cpu.reg(rn);

    let val = a.wrapping_sub(b);

    cpu.set_reg(dst, val);
}

fn subs<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd  = instruction.rd();
    let rn  = instruction.rn();
    let b   = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 2, true));

    if rd.is_pc() {
        panic!("*S instruction with PC target");
    }

    let a = cpu.reg(rn);

    let val = a.wrapping_sub(b);

    let a_neg = (a as i32) < 0;
    let b_neg = (b as i32) < 0;
    let v_neg = (val as i32) < 0;

    cpu.set_reg(rd, val);

    cpu.set_n(v_neg);
    cpu.set_z(val == 0);
    cpu.set_c(a >= b);
    cpu.set_v((a_neg ^ b_neg) & (a_neg ^ v_neg));
}

fn rsb<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd = instruction.rd();
    let rn = instruction.rn();
    let a  = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 3, false));

    let b = cpu.reg(rn);

    let val = a.wrapping_sub(b);

    cpu.set_reg(rd, val);
}

fn rsbs<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd = instruction.rd();
    let rn = instruction.rn();
    let a  = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 3, true));

    if rd.is_pc() {
        panic!("*S instruction with PC target");
    }

    let b = cpu.reg(rn);

    let val = a.wrapping_sub(b);

    let a_neg = (a as i32) < 0;
    let b_neg = (b as i32) < 0;
    let v_neg = (val as i32) < 0;

    cpu.set_reg(rd, val);

    cpu.set_n(v_neg);
    cpu.set_z(val == 0);
    cpu.set_c(a >= b);
    cpu.set_v((a_neg ^ b_neg) & (a_neg ^ v_neg));
}

fn add<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd = instruction.rd();
    let rn = instruction.rn();
    let b  = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 4, false));

    let a = cpu.reg(rn);

    let val = a.wrapping_add(b);

    cpu.set_reg(rd, val);
}

fn adds<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd = instruction.rd();
    let rn = instruction.rn();
    let b  = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 4, true));

    if rd.is_pc() {
        panic!("*S instruction with PC target");
    }

    let a = cpu.reg(rn);

    let (val, c) = a.overflowing_add(b);

    let a_neg = (a as i32) < 0;
    let b_neg = (b as i32) < 0;
    let v_neg = (val as i32) < 0;

    cpu.set_reg(rd, val);

    cpu.set_n(v_neg);
    cpu.set_z(val == 0);
    cpu.set_c(c);
    cpu.set_v((a_neg == b_neg) & (a_neg ^ v_neg));
}

fn adc<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let dst = instruction.rd();
    let rn  = instruction.rn();
    let c   = cpu.c() as u32;
    let b   = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 5, false));

    let a = cpu.reg(rn);

    let val = a.wrapping_add(b).wrapping_add(c);

    cpu.set_reg(dst, val);
}

fn tst<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rn     = instruction.rn();
    let rd     = instruction.rd();
    let (b, c) = M::value_carry(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 8, true));

    if rd != RegisterIndex(0) {
        // "should be zero"
        panic!("TST instruction with non-0 Rd");
    }

    let a = cpu.reg(rn);

    let val = a & b;

    cpu.set_n((val as i32) < 0);
    cpu.set_z(val == 0);
    cpu.set_c(c);
}

fn teq<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rn     = instruction.rn();
    let rd     = instruction.rd();
    let (b, c) = M::value_carry(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 9, true));

    if rd != RegisterIndex(0) {
        // "should be zero"
        panic!("TEQ instruction with non-0 Rd");
    }

    let a = cpu.reg(rn);

    let val = a ^ b;

    cpu.set_n((val as i32) < 0);
    cpu.set_z(val == 0);
    cpu.set_c(c);
}

fn cmp<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rn  = instruction.rn();
    let rd  = instruction.rd();
    let b   = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 10, true));

    if rd != RegisterIndex(0) {
        // "should be zero"
        panic!("CMP instruction with non-0 Rd");
    }

    let a = cpu.reg(rn);

    let val = a.wrapping_sub(b);

    let a_neg = (a as i32) < 0;
    let b_neg = (b as i32) < 0;
    let v_neg = (val as i32) < 0;

    cpu.set_n(v_neg);
    cpu.set_z(val == 0);
    cpu.set_c(a >= b);
    cpu.set_v((a_neg ^ b_neg) & (a_neg ^ v_neg));
}

fn cmn<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rn  = instruction.rn();
    let rd  = instruction.rd();
    let b   = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 11, true));

    if rd != RegisterIndex(0) {
        // "should be zero"
        panic!("CMP instruction with non-0 Rd");
    }

    let a = cpu.reg(rn);

    let val = a.wrapping_add(b);

    let a_neg = (a as i32) < 0;
    let b_neg = (b as i32) < 0;
    let v_neg = (val as i32) < 0;

    cpu.set_n(v_neg);
    cpu.set_z(val == 0);
    cpu.set_c(a >= b);
    cpu.set_v((a_neg ^ b_neg) & (a_neg ^ v_neg));
}

fn orr<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd = instruction.rd();
    let rn = instruction.rn();
    let b  = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 12, false));

    let a = cpu.reg(rn);

    let val = a | b;

    cpu.set_reg(rd, val);
}

fn orrs<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd     = instruction.rd();
    let rn     = instruction.rn();
    let (b, c) = M::value_carry(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 12, true));

    if rd.is_pc() {
        panic!("*S instruction with PC target");
    }

    let a = cpu.reg(rn);

    let val = a | b;

    cpu.set_reg(rd, val);

    cpu.set_n((val as i32) < 0);
    cpu.set_z(val == 0);
    cpu.set_c(c);
}

fn mov<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd  = instruction.rd();
    let rn  = instruction.rn();
    let val = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 13, false));

    if rn != RegisterIndex(0) {
        // "should be zero"
        panic!("CMP instruction with non-0 Rn");
    }

    cpu.set_reg(rd, val);
}

fn movs<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd       = instruction.rd();
    let (val, c) = M::value_carry(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 13, true));

    if rd.is_pc() {
        panic!("*S instruction with PC target");
    }

    cpu.set_reg(rd, val);

    cpu.set_n((val as i32) < 0);
    cpu.set_z(val == 0);
    cpu.set_c(c);
}

fn bic<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd  = instruction.rd();
    let rn  = instruction.rn();
    let b   = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 14, false));

    let a = cpu.reg(rn);

    let val = a & !b;

    cpu.set_reg(rd, val);
}

fn bics<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let rd     = instruction.rd();
    let rn     = instruction.rn();
    let (b, c) = M::value_carry(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 14, true));

    if rd.is_pc() {
        panic!("*S instruction with PC target");
    }

    let a = cpu.reg(rn);

    let val = a & !b;

    cpu.set_reg(rd, val);

    cpu.set_n((val as i32) < 0);
    cpu.set_z(val == 0);
    cpu.set_c(c);
}

fn mvn<M>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where M: Mode1Addressing {
    let dst = instruction.rd();
    let rn = instruction.rn();
    let val = M::value(instruction, cpu);

    debug_assert!(M::is_valid(instruction, 15, false));

    if rn != RegisterIndex(0) {
        // "should be zero"
        panic!("MVN instruction with non-0 Rn");
    }

    cpu.set_reg(dst, !val);
}

fn mul<S>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where S: ModeFlag {
    let rm  = instruction.rm();
    let rs  = instruction.rs();
    // This instruction places Rd where Rn usually is
    let rd  = instruction.rn();

    if rd.is_pc() || rm.is_pc() || rs.is_pc() {
        panic!("Unpredictable MUL");
    }

    let val = cpu.reg(rm).wrapping_mul(cpu.reg(rs));

    cpu.set_reg(rd, val);

    if S::is_set() {
        cpu.set_n((val as i32) < 0);
        cpu.set_z(val == 0);
        // XXX ARM says C flag is UNPREDICTABLE
    }
}

fn mla<S>(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu)
    where S: ModeFlag {
    let rm  = instruction.rm();
    let rs  = instruction.rs();
    // This instruction switches Rd and Rn
    let rd  = instruction.rn();
    let rn  = instruction.rd();

    if rd.is_pc() || rm.is_pc() || rs.is_pc() || rn.is_pc() {
        panic!("Unpredictable MLA");
    }

    let val = cpu.reg(rm).wrapping_mul(cpu.reg(rs)).wrapping_add(cpu.reg(rn));

    cpu.set_reg(rd, val);

    if S::is_set() {
        cpu.set_n((val as i32) < 0);
        cpu.set_z(val == 0);
        // XXX ARM says C flag is UNPREDICTABLE
    }
}

/// Since we can't use boolean varibales as a generic parameter I use
/// this trait to create a boolean "metatype"
trait ModeFlag {
    #[inline(always)]
    fn is_set() -> bool;
    #[inline(always)]
    fn is_clear() -> bool {
        !Self::is_set()
    }
}

struct Set;

impl ModeFlag for Set {
    #[inline(always)]
    fn is_set() -> bool {
        true
    }
}

struct Clear;

impl ModeFlag for Clear {
    #[inline(always)]
    fn is_set() -> bool {
        false
    }
}

/// Addressing mode 2: Load and Store Word or Unsigned Byte
trait Mode2Addressing {
    /// Decode the address and update the registers
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag;

    /// Used to validate that the addressing mode matches the
    /// instruction (useful for debugging).
    fn is_valid<U>(instruction: Instruction, load: bool, byte: bool) -> bool
        where U: ModeFlag;
}

struct Mode2Imm;

impl Mode2Addressing for Mode2Imm {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rn     = instruction.rn();
        let offset = instruction.0 & 0xfff;

        let base = cpu.reg(rn);

        if U::is_set() {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        }
    }

    fn is_valid<U>(instruction: Instruction, load: bool, byte: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0101 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 22) & 1) == byte as u32 &&
            ((i >> 23) & 1) == U::is_set() as u32
    }
}

struct Mode2ImmPre;

impl Mode2Addressing for Mode2ImmPre {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rd     = instruction.rd();
        let rn     = instruction.rn();
        let offset = instruction.0 & 0xfff;

        if rn.is_pc() {
            // Unpredictable
            panic!("PC pre-indexed");
        }

        if rd == rn {
            // Unpredictable
            panic!("Writeback indexing with Rd == Rn");
        }

        let base = cpu.reg(rn);

        let addr =
            if U::is_set() {
                base.wrapping_add(offset)
            } else {
                base.wrapping_sub(offset)
            };

        // Pre index
        cpu.set_reg(rn, addr);

        addr
    }

    fn is_valid<U>(instruction: Instruction, load: bool, byte: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0101 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 22) & 1) == byte as u32 &&
            ((i >> 23) & 1) == U::is_set() as u32
    }
}

struct Mode2ImmPost;

impl Mode2Addressing for Mode2ImmPost {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rd     = instruction.rd();
        let rn     = instruction.rn();
        let offset = instruction.0 & 0xfff;

        if rn.is_pc() {
            // Unpredictable
            panic!("PC post-indexed");
        }

        if rd == rn {
            // Unpredictable
            panic!("Writeback indexing with Rd == Rn");
        }

        let base = cpu.reg(rn);

        let addr =
            if U::is_set() {
                base.wrapping_add(offset)
            } else {
                base.wrapping_sub(offset)
            };

        // Post index
        cpu.set_reg(rn, addr);

        base
    }

    fn is_valid<U>(instruction: Instruction, load: bool, byte: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0100 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 22) & 1) == byte as u32 &&
            ((i >> 23) & 1) == U::is_set() as u32
    }
}

struct Mode2LslReg;

impl Mode2Addressing for Mode2LslReg {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rn    = instruction.rn();
        let rm    = instruction.rm();
        let shift = (instruction.0 >> 7) & 0x1f;

        let offset = cpu.reg(rm) << shift;

        let base = cpu.reg(rn);

        if U::is_set() {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        }
    }

    fn is_valid<U>(instruction: Instruction, load: bool, byte: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0111 &&
            ((i >> 21) & 1) == 0 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 22) & 1) == byte as u32 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 4) & 7) == 0
    }
}

struct Mode2LslRegPre;

impl Mode2Addressing for Mode2LslRegPre {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rn    = instruction.rn();
        let rm    = instruction.rm();
        let shift = (instruction.0 >> 7) & 0x1f;

        let offset = cpu.reg(rm) << shift;

        let base = cpu.reg(rn);

        let addr =
            if U::is_set() {
                base.wrapping_add(offset)
            } else {
                base.wrapping_sub(offset)
            };

        cpu.set_reg(rn, addr);

        addr
    }

    fn is_valid<U>(instruction: Instruction, load: bool, byte: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0111 &&
            ((i >> 21) & 1) == 1 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 22) & 1) == byte as u32 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 4) & 7) == 0
    }
}

fn ldr<M, U>(instruction: Instruction, debugger: &mut Debugger, cpu: &mut Cpu)
    where M: Mode2Addressing, U: ModeFlag {
    let rd   = instruction.rd();
    let addr = M::address::<U>(instruction, cpu);

    debug_assert!(M::is_valid::<U>(instruction, true, false));

    // Bits [1:0] specifies a rightwise rotation by increment of 8
    // bits
    let rot = (addr & 3) * 8;
    let addr = addr & !3;

    let val = cpu.load::<Word>(debugger, addr).rotate_right(rot);

    cpu.set_reg_pc_mask(rd, val);
}

fn str<M, U>(instruction: Instruction, debugger: &mut Debugger, cpu: &mut Cpu)
    where M: Mode2Addressing, U: ModeFlag {
    let rd   = instruction.rd();
    let addr = M::address::<U>(instruction, cpu);

    debug_assert!(M::is_valid::<U>(instruction, false, false));

    if rd.is_pc() {
        // Implementation defined
        panic!("PC stored in STR");
    }

    let val = cpu.reg(rd);

    cpu.store::<Word>(debugger, addr, val);
}

fn ldrb<M, U>(instruction: Instruction, debugger: &mut Debugger, cpu: &mut Cpu)
    where M: Mode2Addressing, U: ModeFlag {
    let rd   = instruction.rd();
    let addr = M::address::<U>(instruction, cpu);

    debug_assert!(M::is_valid::<U>(instruction, true, true));

    let val = cpu.load::<Byte>(debugger, addr);

    cpu.set_reg_pc_mask(rd, val as u32);
}

fn strb<M, U>(instruction: Instruction, debugger: &mut Debugger, cpu: &mut Cpu)
    where M: Mode2Addressing, U: ModeFlag {
    let rd   = instruction.rd();
    let addr = M::address::<U>(instruction, cpu);

    debug_assert!(M::is_valid::<U>(instruction, false, true));

    if rd.is_pc() {
        // I think this is actually allowed and should store
        // cur_instruction + 8 since A2.4.3 only mentions STR and STM
        panic!("PC stored in STRB");
    }

    let val = cpu.reg(rd);

    cpu.store::<Byte>(debugger, addr, val);
}

/// Addressing mode 3: Miscellaneous Loads and Stores
trait Mode3Addressing {
    /// Decode the address and update the registers
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag;

    /// Used to validate that the addressing mode matches the
    /// instruction (useful for debugging).
    fn is_valid<U>(instruction: Instruction,
                   load: bool,
                   byte: bool,
                   signed: bool) -> bool
        where U: ModeFlag;
}

struct Mode3Imm;

impl Mode3Addressing for Mode3Imm {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rn = instruction.rn();
        let hi = (instruction.0 >> 8) & 0xf;
        let lo = instruction.0 & 0xf;

        let offset = (hi << 4) | lo;

        let base = cpu.reg(rn);

        if U::is_set() {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        }
    }

    fn is_valid<U>(instruction: Instruction,
                   load: bool,
                   byte: bool,
                   signed: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0001 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 21) & 3) == 2 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 7) & 1) == 1 &&
            ((i >> 6) & 1) == signed as u32 &&
            ((i >> 5) & 1) == (!byte) as u32 &&
            ((i >> 4) & 1) == 1
    }
}

struct Mode3ImmPre;

impl Mode3Addressing for Mode3ImmPre {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rn = instruction.rn();
        let rd = instruction.rd();
        let hi = (instruction.0 >> 8) & 0xf;
        let lo = instruction.0 & 0xf;

        if rd == rn {
            // Unpredictable
            panic!("Writeback indexing with Rd == Rn");
        }

        let offset = (hi << 4) | lo;

        let base = cpu.reg(rn);

        let addr =
            if U::is_set() {
                base.wrapping_add(offset)
            } else {
                base.wrapping_sub(offset)
            };

        cpu.set_reg(rn, addr);

        addr
    }

    fn is_valid<U>(instruction: Instruction,
                   load: bool,
                   byte: bool,
                   signed: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0001 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 21) & 3) == 3 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 7) & 1) == 1 &&
            ((i >> 6) & 1) == signed as u32 &&
            ((i >> 5) & 1) == (!byte) as u32 &&
            ((i >> 4) & 1) == 1
    }
}

struct Mode3ImmPost;

impl Mode3Addressing for Mode3ImmPost {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rn = instruction.rn();
        let rd = instruction.rd();
        let hi = (instruction.0 >> 8) & 0xf;
        let lo = instruction.0 & 0xf;

        if rd == rn {
            // Unpredictable
            panic!("Writeback indexing with Rd == Rn");
        }

        let offset = (hi << 4) | lo;

        let base = cpu.reg(rn);

        let wb =
            if U::is_set() {
                base.wrapping_add(offset)
            } else {
                base.wrapping_sub(offset)
            };

        cpu.set_reg(rn, wb);

        base
    }

    fn is_valid<U>(instruction: Instruction,
                   load: bool,
                   byte: bool,
                   signed: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0000 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 21) & 3) == 2 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 7) & 1) == 1 &&
            ((i >> 6) & 1) == signed as u32 &&
            ((i >> 5) & 1) == (!byte) as u32 &&
            ((i >> 4) & 1) == 1
    }
}

struct Mode3Reg;

impl Mode3Addressing for Mode3Reg {
    fn address<U>(instruction: Instruction, cpu: &mut Cpu) -> u32
        where U: ModeFlag {
        let rn = instruction.rn();
        let rm = instruction.rm();

        let base = cpu.reg(rn);
        let offset = cpu.reg(rm);

        if U::is_set() {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        }
    }

    fn is_valid<U>(instruction: Instruction,
                   load: bool,
                   byte: bool,
                   signed: bool) -> bool
        where U: ModeFlag {
        let i = instruction.0;

        ((i >> 24) & 0xf) == 0b0001 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 21) & 3) == 0 &&
            ((i >> 20) & 1) == load as u32 &&
            ((i >> 7) & 0x1f) == 1 &&
            ((i >> 6) & 1) == signed as u32 &&
            ((i >> 5) & 1) == (!byte) as u32 &&
            ((i >> 4) & 1) == 1
    }
}

fn ldrh<M, U>(instruction: Instruction, debugger: &mut Debugger, cpu: &mut Cpu)
    where M: Mode3Addressing, U: ModeFlag {
    let rd   = instruction.rd();
    let addr = M::address::<U>(instruction, cpu);

    debug_assert!(M::is_valid::<U>(instruction, true, false, false));

    let val = cpu.load::<HalfWord>(debugger, addr);

    cpu.set_reg(rd, val as u32)
}

fn ldrsh<M, U>(instruction: Instruction, debugger: &mut Debugger, cpu: &mut Cpu)
    where M: Mode3Addressing, U: ModeFlag {
    let rd   = instruction.rd();
    let addr = M::address::<U>(instruction, cpu);

    debug_assert!(M::is_valid::<U>(instruction, true, false, true));

    let val = cpu.load::<HalfWord>(debugger, addr) as i16;

    cpu.set_reg(rd, val as u32)
}

fn strh<M, U>(instruction: Instruction, debugger: &mut Debugger, cpu: &mut Cpu)
    where M: Mode3Addressing, U: ModeFlag {
    let rd   = instruction.rd();
    let addr = M::address::<U>(instruction, cpu);

    debug_assert!(M::is_valid::<U>(instruction, false, false, false));

    let val = cpu.reg(rd);

    cpu.store::<HalfWord>(debugger, addr, val);
}

fn ldrsb<M, U>(instruction: Instruction, debugger: &mut Debugger, cpu: &mut Cpu)
    where M: Mode3Addressing, U: ModeFlag {
    let rd   = instruction.rd();
    let addr = M::address::<U>(instruction, cpu);

    debug_assert!(M::is_valid::<U>(instruction, true, true, true));

    let val = cpu.load::<Byte>(debugger, addr) as i8;

    cpu.set_reg(rd, val as u32)
}

/// LDM/STM start address and WriteBack value
fn mode4_start_wb<U, P>(base: u32, list: u32) -> (u32, u32)
    where U: ModeFlag, P: ModeFlag {
    let list_len = list.count_ones();

    let len_bytes = list_len * 4;

    if U::is_set() {
        let start_addr =
            if P::is_set() {
                base + 4
            } else {
                base
            };

        (start_addr, base.wrapping_add(len_bytes))
    } else {
        let start_addr =
            if P::is_set() {
                base.wrapping_sub(len_bytes)
            } else {
                base.wrapping_sub(len_bytes) + 4
            };

        (start_addr, base.wrapping_sub(len_bytes))
    }
}

fn ldm<U, P, W>(instruction: Instruction,
                debugger: &mut Debugger,
                cpu: &mut Cpu)
    where U: ModeFlag, P: ModeFlag, W: ModeFlag {

    let rn   = instruction.rn();
    let list = instruction.register_list();

    let base_in_list = (list & (1 << rn.0)) != 0;

    debug_assert!({
        let i = instruction.0;

        ((i >> 25) & 7) == 0b100 &&
            ((i >> 24) & 1) == P::is_set() as u32 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 22) & 1) == 0 &&
            ((i >> 21) & 1) == W::is_set() as u32 &&
            ((i >> 20) & 1) == 1 as u32
    });

    if list == 0 || rn.is_pc() ||
        (W::is_set() && base_in_list) {
        panic!("Unpredictable LDM");
    }

    let base = cpu.reg(rn);

    let (mut addr, wb) = mode4_start_wb::<U, P>(base, list);

    for i in 0..16 {
        if ((list >> i) & 1) != 0 {
            let reg = RegisterIndex(i);

            let val = cpu.load::<Word>(debugger, addr);

            cpu.set_reg_pc_mask(reg, val);

            addr = addr.wrapping_add(4);
        }
    }

    if W::is_set() {
        cpu.set_reg(rn, wb);
    }
}

// This opcode can have two meanings depending on whether PC is
// specified in the register list or not.
//
// If PC is set then it's LDM(3) it loads the registers normally and
// then copies the SPSR into the CPSR.
//
// If PC is missing then it's LDM(2) and it loads *user mode*
// registers.
fn ldms<U, P, W>(instruction: Instruction,
                 debugger: &mut Debugger,
                 cpu: &mut Cpu)
    where U: ModeFlag, P: ModeFlag, W: ModeFlag {

    let rn   = instruction.rn();
    let list = instruction.register_list();

    let base_in_list = (list & (1 << rn.0)) != 0;

    debug_assert!({
        let i = instruction.0;

        ((i >> 25) & 7) == 0b100 &&
            ((i >> 24) & 1) == P::is_set() as u32 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 22) & 1) == 1 &&
            ((i >> 21) & 1) == W::is_set() as u32 &&
            ((i >> 20) & 1) == 1 as u32
    });

    if list == 0 || rn.is_pc() ||
        (W::is_set() && base_in_list) {
        panic!("Unpredictable LDM");
    }

    // The presence of PC in the list tells us which instruction this
    // is
    let load_spsr = (list & (1 << 15)) != 0;

    if !load_spsr {
        debug_assert!(W::is_clear());
    }

    let base = cpu.reg(rn);

    let (mut addr, wb) = mode4_start_wb::<U, P>(base, list);

    let mut pc = 0;

    for i in 0..16 {
        if ((list >> i) & 1) != 0 {
            let reg = RegisterIndex(i);

            let val = cpu.load::<Word>(debugger, addr);

            if load_spsr {
                if i == 15 {
                    // Don't load the PC just now, we also need to
                    // restore the SPSR *but* we want to wait until
                    // the writeback is handled, otherwise we might
                    // update a register in the wrong mode.
                    pc = cpu.load::<Word>(debugger, addr);
                } else {
                    cpu.set_reg(reg, val);
                }
            } else {
                // XXX Implement user-mode loading
                unimplemented!();
            }

            addr = addr.wrapping_add(4);
        }
    }

    if W::is_set() {
        cpu.set_reg(rn, wb);
    }

    if load_spsr {
        let spsr = cpu.spsr();

        cpu.set_pc_cpsr(pc, spsr);
    }
}

fn stm<U, P, W>(instruction: Instruction,
                debugger: &mut Debugger,
                cpu: &mut Cpu)
    where U: ModeFlag, P: ModeFlag, W: ModeFlag {
    let rn   = instruction.rn();
    let list = instruction.register_list();

    let base_in_list = (list & (1 << rn.0)) != 0;

    debug_assert!({
        let i = instruction.0;

        ((i >> 25) & 7) == 0b100 &&
            ((i >> 24) & 1) == P::is_set() as u32 &&
            ((i >> 23) & 1) == U::is_set() as u32 &&
            ((i >> 22) & 1) == 0 &&
            ((i >> 21) & 1) == W::is_set() as u32 &&
            ((i >> 20) & 1) == 0 as u32
    });

    if list == 0 || rn.is_pc() ||
        (W::is_set() && base_in_list) {
        panic!("Unpredictable LDM");
    }

    let pc_in_list = (list & (1 << 15)) != 0;

    if pc_in_list {
        panic!("Implementation-defined STM");
    }

    let base = cpu.reg(rn);

    let (mut addr, wb) = mode4_start_wb::<U, P>(base, list);

    let mut first = true;

    for i in 0..16 {
        if ((list >> i) & 1) != 0 {
            let reg = RegisterIndex(i);

            if W::is_set() && reg == rn && !first {
                panic!("Unpredictable STM");
            }

            let val = cpu.reg(reg);
            cpu.store::<Word>(debugger, addr, val);

            addr = addr.wrapping_add(4);

            first = false;
        }
    }

    if W::is_set() {
        cpu.set_reg(rn, wb);
    }
}

fn mrs_cpsr(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu) {
    let rd = instruction.rd();

    if (instruction.0 & 0xf0fff) != 0xf0000 {
        panic!("Invalid MRS instruction {}", instruction);
    }

    let cpsr = cpu.cpsr();

    cpu.set_reg(rd, cpsr);
}

fn msr_cpsr(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu) {
    let rm   = instruction.rm();
    let mask = instruction.msr_field_mask();

    if (instruction.0 & 0xff00) != 0xf000 {
        panic!("Invalid MSR instruction {}", instruction);
    }

    let val = cpu.reg(rm);

    cpu.msr_cpsr(val, mask);
}

fn bx(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu) {
    let rm = instruction.rm();

    if (instruction.0 & 0xfff00) != 0xfff00 {
        // "should be one"
        panic!("Invalid BX instruction {}", instruction);
    }

    let target = cpu.reg(rm);

    // If bit 0 of the target is set we switch to Thumb mode
    let thumb = (target & 1) != 0;
    let address = target & !1;

    cpu.set_pc_thumb(address, thumb);
}

fn mrs_spsr(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu) {
    let rd = instruction.rd();

    if rd.is_pc() || (instruction.0 & 0xf0fff) != 0xf0000 {
        panic!("Invalid MSR instruction {}", instruction);
    }

    let val = cpu.spsr();

    cpu.set_reg(rd, val);
}

fn b(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu) {
    let offset = instruction.branch_imm_offset();

    let pc = cpu.reg(RegisterIndex(15)).wrapping_add(offset);

    cpu.set_pc(pc);
}

fn bl(instruction: Instruction, _: &mut Debugger, cpu: &mut Cpu) {
    let offset = instruction.branch_imm_offset();

    let pc = cpu.registers[15].wrapping_add(offset);

    let ra = cpu.next_pc;

    cpu.set_reg(RegisterIndex(14), ra);

    cpu.set_pc(pc);
}

fn swi(_: Instruction, _: &mut Debugger, cpu: &mut Cpu) {
    cpu.swi();
}

static OPCODE_LUT: [fn (Instruction, &mut Debugger, &mut Cpu); 4096] = [
    // 0x000
    and::<Mode1LslImm>, and::<Mode1LslReg>,
    and::<Mode1LsrImm>, and::<Mode1LsrReg>,
    and::<Mode1AsrImm>, and::<Mode1AsrReg>,
    and::<Mode1RorImm>, and::<Mode1RorReg>,
    and::<Mode1LslImm>, mul::<Clear>,
    and::<Mode1LsrImm>, unimplemented,
    and::<Mode1AsrImm>, unimplemented,
    unimplemented, unimplemented,

    // 0x010
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, mul::<Set>, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x020
    eor::<Mode1LslImm>, eor::<Mode1LslReg>,
    eor::<Mode1LsrImm>, eor::<Mode1LsrReg>,
    eor::<Mode1AsrImm>, eor::<Mode1AsrReg>,
    eor::<Mode1RorImm>, eor::<Mode1RorReg>,
    eor::<Mode1LslImm>, mla::<Clear>,
    eor::<Mode1LsrImm>, unimplemented,
    eor::<Mode1AsrImm>, unimplemented,
    unimplemented, unimplemented,

    // 0x030
    unimplemented, unimplemented, unimplemented, unimplemented,
    eors::<Mode1AsrImm>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    eors::<Mode1AsrImm>, unimplemented, unimplemented, unimplemented,

    // 0x040
    sub::<Mode1LslImm>, unimplemented, sub::<Mode1LsrImm>, unimplemented,
    sub::<Mode1AsrImm>, unimplemented, unimplemented, unimplemented,
    sub::<Mode1LslImm>, unimplemented, sub::<Mode1LsrImm>, unimplemented,
    sub::<Mode1AsrImm>, unimplemented, unimplemented, unimplemented,

    // 0x050
    subs::<Mode1LslImm>, unimplemented, subs::<Mode1LsrImm>, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    subs::<Mode1LslImm>, unimplemented, subs::<Mode1LsrImm>, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x060
    rsb::<Mode1LslImm>, unimplemented, rsb::<Mode1LsrImm>, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    rsb::<Mode1LslImm>, unimplemented, rsb::<Mode1LsrImm>, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x070
    rsbs::<Mode1LslImm>, unimplemented, rsbs::<Mode1LsrImm>, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    rsbs::<Mode1LslImm>, unimplemented, rsbs::<Mode1LsrImm>, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x080
    add::<Mode1LslImm>, add::<Mode1LslReg>,
    add::<Mode1LsrImm>, add::<Mode1LsrReg>,
    add::<Mode1AsrImm>, add::<Mode1AsrReg>,
    add::<Mode1RorImm>, add::<Mode1RorReg>,
    add::<Mode1LslImm>, unimplemented,
    add::<Mode1LsrImm>, unimplemented,
    add::<Mode1AsrImm>, unimplemented,
    unimplemented, unimplemented,

    // 0x090
    adds::<Mode1LslImm>, adds::<Mode1LslReg>,
    adds::<Mode1LsrImm>, adds::<Mode1LsrReg>,
    adds::<Mode1AsrImm>, adds::<Mode1AsrReg>,
    adds::<Mode1RorImm>, adds::<Mode1RorReg>,
    adds::<Mode1LslImm>, unimplemented,
    adds::<Mode1LsrImm>, unimplemented,
    adds::<Mode1AsrImm>, unimplemented,
    unimplemented, unimplemented,

    // 0x0a0
    adc::<Mode1LslImm>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    adc::<Mode1LslImm>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x0b0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x0c0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, strh::<Mode3ImmPost, Set>,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x0d0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, ldrh::<Mode3ImmPost, Set>,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x0e0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x0f0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x100
    mrs_cpsr, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x110
    tst::<Mode1LslImm>, tst::<Mode1LslReg>,
    tst::<Mode1LsrImm>, tst::<Mode1LsrReg>,
    tst::<Mode1AsrImm>, tst::<Mode1AsrReg>,
    tst::<Mode1RorImm>, tst::<Mode1RorReg>,
    tst::<Mode1LslImm>, unimplemented,
    tst::<Mode1LsrImm>, unimplemented,
    tst::<Mode1AsrImm>, unimplemented,
    tst::<Mode1RorImm>, unimplemented,

    // 0x120
    msr_cpsr, bx, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x130
    teq::<Mode1LslImm>, unimplemented, teq::<Mode1LsrImm>, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    teq::<Mode1LslImm>, unimplemented, teq::<Mode1LsrImm>, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x140
    mrs_spsr, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, strh::<Mode3Imm, Clear>,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x150
    cmp::<Mode1LslImm>, unimplemented,
    cmp::<Mode1LsrImm>, unimplemented,
    cmp::<Mode1AsrImm>, unimplemented,
    cmp::<Mode1RorImm>, unimplemented,
    cmp::<Mode1LslImm>, unimplemented,
    cmp::<Mode1LsrImm>, ldrh::<Mode3Imm, Clear>,
    cmp::<Mode1AsrImm>, unimplemented,
    unimplemented, ldrsh::<Mode3Imm, Clear>,

    // 0x160
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, strh::<Mode3ImmPre, Clear>,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x170
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x180
    orr::<Mode1LslImm>, orr::<Mode1LslReg>,
    orr::<Mode1LsrImm>, orr::<Mode1LsrReg>,
    orr::<Mode1AsrImm>, orr::<Mode1AsrReg>,
    orr::<Mode1RorImm>, orr::<Mode1RorReg>,
    orr::<Mode1LslImm>, unimplemented,
    orr::<Mode1LsrImm>, strh::<Mode3Reg, Set>,
    orr::<Mode1AsrImm>, unimplemented,
    orr::<Mode1RorImm>, unimplemented,

    // 0x190
    orrs::<Mode1LslImm>, orrs::<Mode1LslReg>,
    orrs::<Mode1LsrImm>, orrs::<Mode1LsrReg>,
    orrs::<Mode1AsrImm>, orrs::<Mode1AsrReg>,
    orrs::<Mode1RorImm>, orrs::<Mode1RorReg>,
    orrs::<Mode1LslImm>, unimplemented,
    orrs::<Mode1LsrImm>, ldrh::<Mode3Reg, Set>,
    orrs::<Mode1AsrImm>, ldrsb::<Mode3Reg, Set>,
    unimplemented, ldrsh::<Mode3Reg, Set>,

    // 0x1a0
    mov::<Mode1LslImm>, mov::<Mode1LslReg>,
    mov::<Mode1LsrImm>, mov::<Mode1LsrReg>,
    mov::<Mode1AsrImm>, mov::<Mode1AsrReg>,
    mov::<Mode1RorImm>, mov::<Mode1RorReg>,
    mov::<Mode1LslImm>, unimplemented,
    mov::<Mode1LsrImm>, unimplemented,
    mov::<Mode1AsrImm>, unimplemented,
    unimplemented, unimplemented,

    // 0x1b0
    movs::<Mode1LslImm>, movs::<Mode1LslReg>,
    movs::<Mode1LsrImm>, movs::<Mode1LsrReg>,
    movs::<Mode1AsrImm>, unimplemented,
    movs::<Mode1RorImm>, unimplemented,
    movs::<Mode1LslImm>, unimplemented,
    movs::<Mode1LsrImm>, unimplemented,
    movs::<Mode1AsrImm>, unimplemented,
    unimplemented, unimplemented,

    // 0x1c0
    bic::<Mode1LslImm>, bic::<Mode1LslReg>,
    bic::<Mode1LsrImm>, bic::<Mode1LsrReg>,
    bic::<Mode1AsrImm>, bic::<Mode1AsrReg>,
    bic::<Mode1RorImm>, bic::<Mode1RorReg>,
    bic::<Mode1LslImm>, unimplemented,
    bic::<Mode1LsrImm>, strh::<Mode3Imm, Set>,
    bic::<Mode1AsrImm>, unimplemented,
    unimplemented, unimplemented,

    // 0x1d0
    bics::<Mode1LslImm>, bics::<Mode1LslReg>,
    unimplemented, unimplemented,
    unimplemented, unimplemented,
    unimplemented, unimplemented,
    bics::<Mode1LslImm>, unimplemented,
    unimplemented, ldrh::<Mode3Imm, Set>,
    unimplemented, ldrsb::<Mode3Imm, Set>,
    unimplemented, ldrsh::<Mode3Imm, Set>,

    // 0x1e0
    mvn::<Mode1LslImm>, mvn::<Mode1LslReg>,
    mvn::<Mode1LsrImm>, mvn::<Mode1LsrReg>,
    mvn::<Mode1AsrImm>, unimplemented,
    mvn::<Mode1RorImm>, unimplemented,
    mvn::<Mode1LslImm>, unimplemented,
    mvn::<Mode1LsrImm>, unimplemented,
    mvn::<Mode1AsrImm>, unimplemented,
    unimplemented, unimplemented,

    // 0x1f0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, ldrh::<Mode3ImmPre, Set>,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x200
    and::<Mode1Imm>, and::<Mode1Imm>, and::<Mode1Imm>, and::<Mode1Imm>,
    and::<Mode1Imm>, and::<Mode1Imm>, and::<Mode1Imm>, and::<Mode1Imm>,
    and::<Mode1Imm>, and::<Mode1Imm>, and::<Mode1Imm>, and::<Mode1Imm>,
    and::<Mode1Imm>, and::<Mode1Imm>, and::<Mode1Imm>, and::<Mode1Imm>,

    // 0x210
    ands::<Mode1Imm>, ands::<Mode1Imm>, ands::<Mode1Imm>, ands::<Mode1Imm>,
    ands::<Mode1Imm>, ands::<Mode1Imm>, ands::<Mode1Imm>, ands::<Mode1Imm>,
    ands::<Mode1Imm>, ands::<Mode1Imm>, ands::<Mode1Imm>, ands::<Mode1Imm>,
    ands::<Mode1Imm>, ands::<Mode1Imm>, ands::<Mode1Imm>, ands::<Mode1Imm>,

    // 0x220
    eor::<Mode1Imm>, eor::<Mode1Imm>, eor::<Mode1Imm>, eor::<Mode1Imm>,
    eor::<Mode1Imm>, eor::<Mode1Imm>, eor::<Mode1Imm>, eor::<Mode1Imm>,
    eor::<Mode1Imm>, eor::<Mode1Imm>, eor::<Mode1Imm>, eor::<Mode1Imm>,
    eor::<Mode1Imm>, eor::<Mode1Imm>, eor::<Mode1Imm>, eor::<Mode1Imm>,

    // 0x230
    eors::<Mode1Imm>, eors::<Mode1Imm>, eors::<Mode1Imm>, eors::<Mode1Imm>,
    eors::<Mode1Imm>, eors::<Mode1Imm>, eors::<Mode1Imm>, eors::<Mode1Imm>,
    eors::<Mode1Imm>, eors::<Mode1Imm>, eors::<Mode1Imm>, eors::<Mode1Imm>,
    eors::<Mode1Imm>, eors::<Mode1Imm>, eors::<Mode1Imm>, eors::<Mode1Imm>,

    // 0x240
    sub::<Mode1Imm>, sub::<Mode1Imm>, sub::<Mode1Imm>, sub::<Mode1Imm>,
    sub::<Mode1Imm>, sub::<Mode1Imm>, sub::<Mode1Imm>, sub::<Mode1Imm>,
    sub::<Mode1Imm>, sub::<Mode1Imm>, sub::<Mode1Imm>, sub::<Mode1Imm>,
    sub::<Mode1Imm>, sub::<Mode1Imm>, sub::<Mode1Imm>, sub::<Mode1Imm>,

    // 0x250
    subs::<Mode1Imm>, subs::<Mode1Imm>, subs::<Mode1Imm>, subs::<Mode1Imm>,
    subs::<Mode1Imm>, subs::<Mode1Imm>, subs::<Mode1Imm>, subs::<Mode1Imm>,
    subs::<Mode1Imm>, subs::<Mode1Imm>, subs::<Mode1Imm>, subs::<Mode1Imm>,
    subs::<Mode1Imm>, subs::<Mode1Imm>, subs::<Mode1Imm>, subs::<Mode1Imm>,

    // 0x260
    rsb::<Mode1Imm>, rsb::<Mode1Imm>, rsb::<Mode1Imm>, rsb::<Mode1Imm>,
    rsb::<Mode1Imm>, rsb::<Mode1Imm>, rsb::<Mode1Imm>, rsb::<Mode1Imm>,
    rsb::<Mode1Imm>, rsb::<Mode1Imm>, rsb::<Mode1Imm>, rsb::<Mode1Imm>,
    rsb::<Mode1Imm>, rsb::<Mode1Imm>, rsb::<Mode1Imm>, rsb::<Mode1Imm>,

    // 0x270
    rsbs::<Mode1Imm>, rsbs::<Mode1Imm>, rsbs::<Mode1Imm>, rsbs::<Mode1Imm>,
    rsbs::<Mode1Imm>, rsbs::<Mode1Imm>, rsbs::<Mode1Imm>, rsbs::<Mode1Imm>,
    rsbs::<Mode1Imm>, rsbs::<Mode1Imm>, rsbs::<Mode1Imm>, rsbs::<Mode1Imm>,
    rsbs::<Mode1Imm>, rsbs::<Mode1Imm>, rsbs::<Mode1Imm>, rsbs::<Mode1Imm>,

    // 0x280
    add::<Mode1Imm>, add::<Mode1Imm>, add::<Mode1Imm>, add::<Mode1Imm>,
    add::<Mode1Imm>, add::<Mode1Imm>, add::<Mode1Imm>, add::<Mode1Imm>,
    add::<Mode1Imm>, add::<Mode1Imm>, add::<Mode1Imm>, add::<Mode1Imm>,
    add::<Mode1Imm>, add::<Mode1Imm>, add::<Mode1Imm>, add::<Mode1Imm>,

    // 0x290
    adds::<Mode1Imm>, adds::<Mode1Imm>, adds::<Mode1Imm>, adds::<Mode1Imm>,
    adds::<Mode1Imm>, adds::<Mode1Imm>, adds::<Mode1Imm>, adds::<Mode1Imm>,
    adds::<Mode1Imm>, adds::<Mode1Imm>, adds::<Mode1Imm>, adds::<Mode1Imm>,
    adds::<Mode1Imm>, adds::<Mode1Imm>, adds::<Mode1Imm>, adds::<Mode1Imm>,

    // 0x2a0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x2b0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x2c0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x2d0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x2e0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x2f0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x300
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x310
    tst::<Mode1Imm>, tst::<Mode1Imm>, tst::<Mode1Imm>, tst::<Mode1Imm>,
    tst::<Mode1Imm>, tst::<Mode1Imm>, tst::<Mode1Imm>, tst::<Mode1Imm>,
    tst::<Mode1Imm>, tst::<Mode1Imm>, tst::<Mode1Imm>, tst::<Mode1Imm>,
    tst::<Mode1Imm>, tst::<Mode1Imm>, tst::<Mode1Imm>, tst::<Mode1Imm>,

    // 0x320
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x330
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x340
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x350
    cmp::<Mode1Imm>, cmp::<Mode1Imm>, cmp::<Mode1Imm>, cmp::<Mode1Imm>,
    cmp::<Mode1Imm>, cmp::<Mode1Imm>, cmp::<Mode1Imm>, cmp::<Mode1Imm>,
    cmp::<Mode1Imm>, cmp::<Mode1Imm>, cmp::<Mode1Imm>, cmp::<Mode1Imm>,
    cmp::<Mode1Imm>, cmp::<Mode1Imm>, cmp::<Mode1Imm>, cmp::<Mode1Imm>,

    // 0x360
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x370
    cmn::<Mode1Imm>, cmn::<Mode1Imm>, cmn::<Mode1Imm>, cmn::<Mode1Imm>,
    cmn::<Mode1Imm>, cmn::<Mode1Imm>, cmn::<Mode1Imm>, cmn::<Mode1Imm>,
    cmn::<Mode1Imm>, cmn::<Mode1Imm>, cmn::<Mode1Imm>, cmn::<Mode1Imm>,
    cmn::<Mode1Imm>, cmn::<Mode1Imm>, cmn::<Mode1Imm>, cmn::<Mode1Imm>,

    // 0x380
    orr::<Mode1Imm>, orr::<Mode1Imm>, orr::<Mode1Imm>, orr::<Mode1Imm>,
    orr::<Mode1Imm>, orr::<Mode1Imm>, orr::<Mode1Imm>, orr::<Mode1Imm>,
    orr::<Mode1Imm>, orr::<Mode1Imm>, orr::<Mode1Imm>, orr::<Mode1Imm>,
    orr::<Mode1Imm>, orr::<Mode1Imm>, orr::<Mode1Imm>, orr::<Mode1Imm>,

    // 0x390
    orrs::<Mode1Imm>, orrs::<Mode1Imm>, orrs::<Mode1Imm>, orrs::<Mode1Imm>,
    orrs::<Mode1Imm>, orrs::<Mode1Imm>, orrs::<Mode1Imm>, orrs::<Mode1Imm>,
    orrs::<Mode1Imm>, orrs::<Mode1Imm>, orrs::<Mode1Imm>, orrs::<Mode1Imm>,
    orrs::<Mode1Imm>, orrs::<Mode1Imm>, orrs::<Mode1Imm>, orrs::<Mode1Imm>,

    // 0x3a0
    mov::<Mode1Imm>, mov::<Mode1Imm>, mov::<Mode1Imm>, mov::<Mode1Imm>,
    mov::<Mode1Imm>, mov::<Mode1Imm>, mov::<Mode1Imm>, mov::<Mode1Imm>,
    mov::<Mode1Imm>, mov::<Mode1Imm>, mov::<Mode1Imm>, mov::<Mode1Imm>,
    mov::<Mode1Imm>, mov::<Mode1Imm>, mov::<Mode1Imm>, mov::<Mode1Imm>,

    // 0x3b0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x3c0
    bic::<Mode1Imm>, bic::<Mode1Imm>, bic::<Mode1Imm>, bic::<Mode1Imm>,
    bic::<Mode1Imm>, bic::<Mode1Imm>, bic::<Mode1Imm>, bic::<Mode1Imm>,
    bic::<Mode1Imm>, bic::<Mode1Imm>, bic::<Mode1Imm>, bic::<Mode1Imm>,
    bic::<Mode1Imm>, bic::<Mode1Imm>, bic::<Mode1Imm>, bic::<Mode1Imm>,

    // 0x3d0
    bics::<Mode1Imm>, bics::<Mode1Imm>, bics::<Mode1Imm>, bics::<Mode1Imm>,
    bics::<Mode1Imm>, bics::<Mode1Imm>, bics::<Mode1Imm>, bics::<Mode1Imm>,
    bics::<Mode1Imm>, bics::<Mode1Imm>, bics::<Mode1Imm>, bics::<Mode1Imm>,
    bics::<Mode1Imm>, bics::<Mode1Imm>, bics::<Mode1Imm>, bics::<Mode1Imm>,

    // 0x3e0
    mvn::<Mode1Imm>, mvn::<Mode1Imm>, mvn::<Mode1Imm>, mvn::<Mode1Imm>,
    mvn::<Mode1Imm>, mvn::<Mode1Imm>, mvn::<Mode1Imm>, mvn::<Mode1Imm>,
    mvn::<Mode1Imm>, mvn::<Mode1Imm>, mvn::<Mode1Imm>, mvn::<Mode1Imm>,
    mvn::<Mode1Imm>, mvn::<Mode1Imm>, mvn::<Mode1Imm>, mvn::<Mode1Imm>,

    // 0x3f0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x400
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x410
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x420
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x430
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x440
    strb::<Mode2ImmPost, Clear>, strb::<Mode2ImmPost, Clear>,
    strb::<Mode2ImmPost, Clear>, strb::<Mode2ImmPost, Clear>,
    strb::<Mode2ImmPost, Clear>, strb::<Mode2ImmPost, Clear>,
    strb::<Mode2ImmPost, Clear>, strb::<Mode2ImmPost, Clear>,
    strb::<Mode2ImmPost, Clear>, strb::<Mode2ImmPost, Clear>,
    strb::<Mode2ImmPost, Clear>, strb::<Mode2ImmPost, Clear>,
    strb::<Mode2ImmPost, Clear>, strb::<Mode2ImmPost, Clear>,
    strb::<Mode2ImmPost, Clear>, strb::<Mode2ImmPost, Clear>,

    // 0x450
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x460
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x470
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x480
    str::<Mode2ImmPost, Set>, str::<Mode2ImmPost, Set>,
    str::<Mode2ImmPost, Set>, str::<Mode2ImmPost, Set>,
    str::<Mode2ImmPost, Set>, str::<Mode2ImmPost, Set>,
    str::<Mode2ImmPost, Set>, str::<Mode2ImmPost, Set>,
    str::<Mode2ImmPost, Set>, str::<Mode2ImmPost, Set>,
    str::<Mode2ImmPost, Set>, str::<Mode2ImmPost, Set>,
    str::<Mode2ImmPost, Set>, str::<Mode2ImmPost, Set>,
    str::<Mode2ImmPost, Set>, str::<Mode2ImmPost, Set>,

    // 0x490
    ldr::<Mode2ImmPost, Set>, ldr::<Mode2ImmPost, Set>,
    ldr::<Mode2ImmPost, Set>, ldr::<Mode2ImmPost, Set>,
    ldr::<Mode2ImmPost, Set>, ldr::<Mode2ImmPost, Set>,
    ldr::<Mode2ImmPost, Set>, ldr::<Mode2ImmPost, Set>,
    ldr::<Mode2ImmPost, Set>, ldr::<Mode2ImmPost, Set>,
    ldr::<Mode2ImmPost, Set>, ldr::<Mode2ImmPost, Set>,
    ldr::<Mode2ImmPost, Set>, ldr::<Mode2ImmPost, Set>,
    ldr::<Mode2ImmPost, Set>, ldr::<Mode2ImmPost, Set>,

    // 0x4a0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x4b0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x4c0
    strb::<Mode2ImmPost, Set>, strb::<Mode2ImmPost, Set>,
    strb::<Mode2ImmPost, Set>, strb::<Mode2ImmPost, Set>,
    strb::<Mode2ImmPost, Set>, strb::<Mode2ImmPost, Set>,
    strb::<Mode2ImmPost, Set>, strb::<Mode2ImmPost, Set>,
    strb::<Mode2ImmPost, Set>, strb::<Mode2ImmPost, Set>,
    strb::<Mode2ImmPost, Set>, strb::<Mode2ImmPost, Set>,
    strb::<Mode2ImmPost, Set>, strb::<Mode2ImmPost, Set>,
    strb::<Mode2ImmPost, Set>, strb::<Mode2ImmPost, Set>,

    // 0x4d0
    ldrb::<Mode2ImmPost, Set>, ldrb::<Mode2ImmPost, Set>,
    ldrb::<Mode2ImmPost, Set>, ldrb::<Mode2ImmPost, Set>,
    ldrb::<Mode2ImmPost, Set>, ldrb::<Mode2ImmPost, Set>,
    ldrb::<Mode2ImmPost, Set>, ldrb::<Mode2ImmPost, Set>,
    ldrb::<Mode2ImmPost, Set>, ldrb::<Mode2ImmPost, Set>,
    ldrb::<Mode2ImmPost, Set>, ldrb::<Mode2ImmPost, Set>,
    ldrb::<Mode2ImmPost, Set>, ldrb::<Mode2ImmPost, Set>,
    ldrb::<Mode2ImmPost, Set>, ldrb::<Mode2ImmPost, Set>,

    // 0x4e0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x4f0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x500
    str::<Mode2Imm, Clear>, str::<Mode2Imm, Clear>,
    str::<Mode2Imm, Clear>, str::<Mode2Imm, Clear>,
    str::<Mode2Imm, Clear>, str::<Mode2Imm, Clear>,
    str::<Mode2Imm, Clear>, str::<Mode2Imm, Clear>,
    str::<Mode2Imm, Clear>, str::<Mode2Imm, Clear>,
    str::<Mode2Imm, Clear>, str::<Mode2Imm, Clear>,
    str::<Mode2Imm, Clear>, str::<Mode2Imm, Clear>,
    str::<Mode2Imm, Clear>, str::<Mode2Imm, Clear>,

    // 0x510
    ldr::<Mode2Imm, Clear>, ldr::<Mode2Imm, Clear>,
    ldr::<Mode2Imm, Clear>, ldr::<Mode2Imm, Clear>,
    ldr::<Mode2Imm, Clear>, ldr::<Mode2Imm, Clear>,
    ldr::<Mode2Imm, Clear>, ldr::<Mode2Imm, Clear>,
    ldr::<Mode2Imm, Clear>, ldr::<Mode2Imm, Clear>,
    ldr::<Mode2Imm, Clear>, ldr::<Mode2Imm, Clear>,
    ldr::<Mode2Imm, Clear>, ldr::<Mode2Imm, Clear>,
    ldr::<Mode2Imm, Clear>, ldr::<Mode2Imm, Clear>,

    // 0x520
    str::<Mode2ImmPre, Clear>, str::<Mode2ImmPre, Clear>,
    str::<Mode2ImmPre, Clear>, str::<Mode2ImmPre, Clear>,
    str::<Mode2ImmPre, Clear>, str::<Mode2ImmPre, Clear>,
    str::<Mode2ImmPre, Clear>, str::<Mode2ImmPre, Clear>,
    str::<Mode2ImmPre, Clear>, str::<Mode2ImmPre, Clear>,
    str::<Mode2ImmPre, Clear>, str::<Mode2ImmPre, Clear>,
    str::<Mode2ImmPre, Clear>, str::<Mode2ImmPre, Clear>,
    str::<Mode2ImmPre, Clear>, str::<Mode2ImmPre, Clear>,

    // 0x530
    ldr::<Mode2ImmPre, Clear>, ldr::<Mode2ImmPre, Clear>,
    ldr::<Mode2ImmPre, Clear>, ldr::<Mode2ImmPre, Clear>,
    ldr::<Mode2ImmPre, Clear>, ldr::<Mode2ImmPre, Clear>,
    ldr::<Mode2ImmPre, Clear>, ldr::<Mode2ImmPre, Clear>,
    ldr::<Mode2ImmPre, Clear>, ldr::<Mode2ImmPre, Clear>,
    ldr::<Mode2ImmPre, Clear>, ldr::<Mode2ImmPre, Clear>,
    ldr::<Mode2ImmPre, Clear>, ldr::<Mode2ImmPre, Clear>,
    ldr::<Mode2ImmPre, Clear>, ldr::<Mode2ImmPre, Clear>,

    // 0x540
    strb::<Mode2Imm, Clear>, strb::<Mode2Imm, Clear>,
    strb::<Mode2Imm, Clear>, strb::<Mode2Imm, Clear>,
    strb::<Mode2Imm, Clear>, strb::<Mode2Imm, Clear>,
    strb::<Mode2Imm, Clear>, strb::<Mode2Imm, Clear>,
    strb::<Mode2Imm, Clear>, strb::<Mode2Imm, Clear>,
    strb::<Mode2Imm, Clear>, strb::<Mode2Imm, Clear>,
    strb::<Mode2Imm, Clear>, strb::<Mode2Imm, Clear>,
    strb::<Mode2Imm, Clear>, strb::<Mode2Imm, Clear>,

    // 0x550
    ldrb::<Mode2Imm, Clear>, ldrb::<Mode2Imm, Clear>,
    ldrb::<Mode2Imm, Clear>, ldrb::<Mode2Imm, Clear>,
    ldrb::<Mode2Imm, Clear>, ldrb::<Mode2Imm, Clear>,
    ldrb::<Mode2Imm, Clear>, ldrb::<Mode2Imm, Clear>,
    ldrb::<Mode2Imm, Clear>, ldrb::<Mode2Imm, Clear>,
    ldrb::<Mode2Imm, Clear>, ldrb::<Mode2Imm, Clear>,
    ldrb::<Mode2Imm, Clear>, ldrb::<Mode2Imm, Clear>,
    ldrb::<Mode2Imm, Clear>, ldrb::<Mode2Imm, Clear>,

    // 0x560
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x570
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x580
    str::<Mode2Imm, Set>, str::<Mode2Imm, Set>,
    str::<Mode2Imm, Set>, str::<Mode2Imm, Set>,
    str::<Mode2Imm, Set>, str::<Mode2Imm, Set>,
    str::<Mode2Imm, Set>, str::<Mode2Imm, Set>,
    str::<Mode2Imm, Set>, str::<Mode2Imm, Set>,
    str::<Mode2Imm, Set>, str::<Mode2Imm, Set>,
    str::<Mode2Imm, Set>, str::<Mode2Imm, Set>,
    str::<Mode2Imm, Set>, str::<Mode2Imm, Set>,

    // 0x590
    ldr::<Mode2Imm, Set>, ldr::<Mode2Imm, Set>,
    ldr::<Mode2Imm, Set>, ldr::<Mode2Imm, Set>,
    ldr::<Mode2Imm, Set>, ldr::<Mode2Imm, Set>,
    ldr::<Mode2Imm, Set>, ldr::<Mode2Imm, Set>,
    ldr::<Mode2Imm, Set>, ldr::<Mode2Imm, Set>,
    ldr::<Mode2Imm, Set>, ldr::<Mode2Imm, Set>,
    ldr::<Mode2Imm, Set>, ldr::<Mode2Imm, Set>,
    ldr::<Mode2Imm, Set>, ldr::<Mode2Imm, Set>,

    // 0x5a0
    str::<Mode2ImmPre, Set>, str::<Mode2ImmPre, Set>,
    str::<Mode2ImmPre, Set>, str::<Mode2ImmPre, Set>,
    str::<Mode2ImmPre, Set>, str::<Mode2ImmPre, Set>,
    str::<Mode2ImmPre, Set>, str::<Mode2ImmPre, Set>,
    str::<Mode2ImmPre, Set>, str::<Mode2ImmPre, Set>,
    str::<Mode2ImmPre, Set>, str::<Mode2ImmPre, Set>,
    str::<Mode2ImmPre, Set>, str::<Mode2ImmPre, Set>,
    str::<Mode2ImmPre, Set>, str::<Mode2ImmPre, Set>,

    // 0x5b0
    ldr::<Mode2ImmPre, Set>, ldr::<Mode2ImmPre, Set>,
    ldr::<Mode2ImmPre, Set>, ldr::<Mode2ImmPre, Set>,
    ldr::<Mode2ImmPre, Set>, ldr::<Mode2ImmPre, Set>,
    ldr::<Mode2ImmPre, Set>, ldr::<Mode2ImmPre, Set>,
    ldr::<Mode2ImmPre, Set>, ldr::<Mode2ImmPre, Set>,
    ldr::<Mode2ImmPre, Set>, ldr::<Mode2ImmPre, Set>,
    ldr::<Mode2ImmPre, Set>, ldr::<Mode2ImmPre, Set>,
    ldr::<Mode2ImmPre, Set>, ldr::<Mode2ImmPre, Set>,

    // 0x5c0
    strb::<Mode2Imm, Set>, strb::<Mode2Imm, Set>,
    strb::<Mode2Imm, Set>, strb::<Mode2Imm, Set>,
    strb::<Mode2Imm, Set>, strb::<Mode2Imm, Set>,
    strb::<Mode2Imm, Set>, strb::<Mode2Imm, Set>,
    strb::<Mode2Imm, Set>, strb::<Mode2Imm, Set>,
    strb::<Mode2Imm, Set>, strb::<Mode2Imm, Set>,
    strb::<Mode2Imm, Set>, strb::<Mode2Imm, Set>,
    strb::<Mode2Imm, Set>, strb::<Mode2Imm, Set>,

    // 0x5d0
    ldrb::<Mode2Imm, Set>, ldrb::<Mode2Imm, Set>,
    ldrb::<Mode2Imm, Set>, ldrb::<Mode2Imm, Set>,
    ldrb::<Mode2Imm, Set>, ldrb::<Mode2Imm, Set>,
    ldrb::<Mode2Imm, Set>, ldrb::<Mode2Imm, Set>,
    ldrb::<Mode2Imm, Set>, ldrb::<Mode2Imm, Set>,
    ldrb::<Mode2Imm, Set>, ldrb::<Mode2Imm, Set>,
    ldrb::<Mode2Imm, Set>, ldrb::<Mode2Imm, Set>,
    ldrb::<Mode2Imm, Set>, ldrb::<Mode2Imm, Set>,

    // 0x5e0
    strb::<Mode2ImmPre, Set>, strb::<Mode2ImmPre, Set>,
    strb::<Mode2ImmPre, Set>, strb::<Mode2ImmPre, Set>,
    strb::<Mode2ImmPre, Set>, strb::<Mode2ImmPre, Set>,
    strb::<Mode2ImmPre, Set>, strb::<Mode2ImmPre, Set>,
    strb::<Mode2ImmPre, Set>, strb::<Mode2ImmPre, Set>,
    strb::<Mode2ImmPre, Set>, strb::<Mode2ImmPre, Set>,
    strb::<Mode2ImmPre, Set>, strb::<Mode2ImmPre, Set>,
    strb::<Mode2ImmPre, Set>, strb::<Mode2ImmPre, Set>,

    // 0x5f0
    ldrb::<Mode2ImmPre, Set>, ldrb::<Mode2ImmPre, Set>,
    ldrb::<Mode2ImmPre, Set>, ldrb::<Mode2ImmPre, Set>,
    ldrb::<Mode2ImmPre, Set>, ldrb::<Mode2ImmPre, Set>,
    ldrb::<Mode2ImmPre, Set>, ldrb::<Mode2ImmPre, Set>,
    ldrb::<Mode2ImmPre, Set>, ldrb::<Mode2ImmPre, Set>,
    ldrb::<Mode2ImmPre, Set>, ldrb::<Mode2ImmPre, Set>,
    ldrb::<Mode2ImmPre, Set>, ldrb::<Mode2ImmPre, Set>,
    ldrb::<Mode2ImmPre, Set>, ldrb::<Mode2ImmPre, Set>,

    // 0x600
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x610
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x620
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x630
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x640
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x650
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x660
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x670
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x680
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x690
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x6a0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x6b0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x6c0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x6d0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x6e0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x6f0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x700
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x710
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x720
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x730
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x740
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x750
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x760
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x770
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x780
    str::<Mode2LslReg, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    str::<Mode2LslReg, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x790
    ldr::<Mode2LslReg, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    ldr::<Mode2LslReg, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x7a0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x7b0
    ldr::<Mode2LslRegPre, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x7c0
    strb::<Mode2LslReg, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    strb::<Mode2LslReg, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x7d0
    ldrb::<Mode2LslReg, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    ldrb::<Mode2LslReg, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x7e0
    strb::<Mode2LslRegPre, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    strb::<Mode2LslRegPre, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x7f0
    ldrb::<Mode2LslRegPre, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    ldrb::<Mode2LslRegPre, Set>, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x800
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x810
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x820
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x830
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x840
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x850
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x860
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x870
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x880
    stm::<Set, Clear, Clear>, stm::<Set, Clear, Clear>,
    stm::<Set, Clear, Clear>, stm::<Set, Clear, Clear>,
    stm::<Set, Clear, Clear>, stm::<Set, Clear, Clear>,
    stm::<Set, Clear, Clear>, stm::<Set, Clear, Clear>,
    stm::<Set, Clear, Clear>, stm::<Set, Clear, Clear>,
    stm::<Set, Clear, Clear>, stm::<Set, Clear, Clear>,
    stm::<Set, Clear, Clear>, stm::<Set, Clear, Clear>,
    stm::<Set, Clear, Clear>, stm::<Set, Clear, Clear>,

    // 0x890
    ldm::<Set, Clear, Clear>, ldm::<Set, Clear, Clear>,
    ldm::<Set, Clear, Clear>, ldm::<Set, Clear, Clear>,
    ldm::<Set, Clear, Clear>, ldm::<Set, Clear, Clear>,
    ldm::<Set, Clear, Clear>, ldm::<Set, Clear, Clear>,
    ldm::<Set, Clear, Clear>, ldm::<Set, Clear, Clear>,
    ldm::<Set, Clear, Clear>, ldm::<Set, Clear, Clear>,
    ldm::<Set, Clear, Clear>, ldm::<Set, Clear, Clear>,
    ldm::<Set, Clear, Clear>, ldm::<Set, Clear, Clear>,

    // 0x8a0
    stm::<Set, Clear, Set>, stm::<Set, Clear, Set>,
    stm::<Set, Clear, Set>, stm::<Set, Clear, Set>,
    stm::<Set, Clear, Set>, stm::<Set, Clear, Set>,
    stm::<Set, Clear, Set>, stm::<Set, Clear, Set>,
    stm::<Set, Clear, Set>, stm::<Set, Clear, Set>,
    stm::<Set, Clear, Set>, stm::<Set, Clear, Set>,
    stm::<Set, Clear, Set>, stm::<Set, Clear, Set>,
    stm::<Set, Clear, Set>, stm::<Set, Clear, Set>,

    // 0x8b0
    ldm::<Set, Clear, Set>, ldm::<Set, Clear, Set>,
    ldm::<Set, Clear, Set>, ldm::<Set, Clear, Set>,
    ldm::<Set, Clear, Set>, ldm::<Set, Clear, Set>,
    ldm::<Set, Clear, Set>, ldm::<Set, Clear, Set>,
    ldm::<Set, Clear, Set>, ldm::<Set, Clear, Set>,
    ldm::<Set, Clear, Set>, ldm::<Set, Clear, Set>,
    ldm::<Set, Clear, Set>, ldm::<Set, Clear, Set>,
    ldm::<Set, Clear, Set>, ldm::<Set, Clear, Set>,

    // 0x8c0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x8d0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x8e0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x8f0
    ldms::<Set, Clear, Set>, ldms::<Set, Clear, Set>,
    ldms::<Set, Clear, Set>, ldms::<Set, Clear, Set>,
    ldms::<Set, Clear, Set>, ldms::<Set, Clear, Set>,
    ldms::<Set, Clear, Set>, ldms::<Set, Clear, Set>,
    ldms::<Set, Clear, Set>, ldms::<Set, Clear, Set>,
    ldms::<Set, Clear, Set>, ldms::<Set, Clear, Set>,
    ldms::<Set, Clear, Set>, ldms::<Set, Clear, Set>,
    ldms::<Set, Clear, Set>, ldms::<Set, Clear, Set>,

    // 0x900
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x910
    ldm::<Clear, Set, Clear>, ldm::<Clear, Set, Clear>,
    ldm::<Clear, Set, Clear>, ldm::<Clear, Set, Clear>,
    ldm::<Clear, Set, Clear>, ldm::<Clear, Set, Clear>,
    ldm::<Clear, Set, Clear>, ldm::<Clear, Set, Clear>,
    ldm::<Clear, Set, Clear>, ldm::<Clear, Set, Clear>,
    ldm::<Clear, Set, Clear>, ldm::<Clear, Set, Clear>,
    ldm::<Clear, Set, Clear>, ldm::<Clear, Set, Clear>,
    ldm::<Clear, Set, Clear>, ldm::<Clear, Set, Clear>,

    // 0x920
    stm::<Clear, Set, Set>, stm::<Clear, Set, Set>,
    stm::<Clear, Set, Set>, stm::<Clear, Set, Set>,
    stm::<Clear, Set, Set>, stm::<Clear, Set, Set>,
    stm::<Clear, Set, Set>, stm::<Clear, Set, Set>,
    stm::<Clear, Set, Set>, stm::<Clear, Set, Set>,
    stm::<Clear, Set, Set>, stm::<Clear, Set, Set>,
    stm::<Clear, Set, Set>, stm::<Clear, Set, Set>,
    stm::<Clear, Set, Set>, stm::<Clear, Set, Set>,

    // 0x930
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x940
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x950
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x960
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x970
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x980
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x990
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x9a0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x9b0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x9c0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x9d0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x9e0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0x9f0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xa00
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa10
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa20
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa30
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa40
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa50
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa60
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa70
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa80
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xa90
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xaa0
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xab0
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xac0
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xad0
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xae0
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xaf0
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,
    b, b, b, b,

    // 0xb00
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb10
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb20
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb30
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb40
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb50
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb60
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb70
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb80
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xb90
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xba0
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xbb0
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xbc0
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xbd0
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xbe0
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xbf0
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,
    bl, bl, bl, bl,

    // 0xc00
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc10
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc20
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc30
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc40
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc50
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc60
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc70
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc80
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xc90
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xca0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xcb0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xcc0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xcd0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xce0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xcf0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd00
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd10
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd20
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd30
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd40
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd50
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd60
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd70
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd80
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xd90
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xda0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xdb0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xdc0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xdd0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xde0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xdf0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe00
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe10
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe20
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe30
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe40
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe50
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe60
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe70
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe80
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xe90
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xea0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xeb0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xec0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xed0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xee0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xef0
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,
    unimplemented, unimplemented, unimplemented, unimplemented,

    // 0xf00
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf10
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf20
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf30
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf40
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf50
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf60
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf70
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf80
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xf90
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xfa0
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xfb0
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xfc0
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xfd0
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xfe0
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,

    // 0xff0
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
    swi, swi, swi, swi,
];
