//! This module consists of functions to create a direct memory access mailbox for passing parameters to the hca
//! and getting output back from the hca during verb calls and functions to execute verb calls.

use byteorder::BigEndian;
use memory::MappedPages;
use num_enum_derive::IntoPrimitive;
use strum_macros::{FromRepr, IntoStaticStr};
use volatile::{Volatile, WriteOnly};
use zerocopy::{FromBytes, U32, U64};

const HCR_BASE: usize = 0x80680;
const HCR_OPMOD_SHIFT: u32 = 12;
const HCR_T_BIT: u32 = 21;
const HCR_E_BIT: u32 = 22;
const HCR_GO_BIT: u32 = 23;
const POLL_TOKEN: u32 = 0xffff;

// this is actually just u16
#[repr(u32)]
#[derive(Debug, IntoPrimitive)]
pub(super) enum Opcode {
    // initialization and general commands
    QueryDevCap = 0x03,
    QueryFw = 0x04,
    QueryAdapter = 0x06,
    InitHca = 0x07,
    CloseHca = 0x08,
    InitPort = 0x09,
    ClosePort = 0x0a,
    QueryHca = 0x0b,
    QueryPort = 0x43,
    SetPort = 0x0c,
    RunFw = 0xff6,
    UnmapIcm = 0xff9,
    MapIcm = 0xffa,
    UnmapIcmAux = 0xffb,
    MapIcmAux = 0xffc,
    UnmapFa = 0xffe,
    SetIcmSize = 0xffd,
    MapFa = 0xfff,

    // TPT commands
    Sw2HwMpt = 0x0d,
    QueryMpt = 0x0e,
    Hw2SwMpt = 0x0f,
    ReadMtt = 0x10,
    WriteMtt = 0x11,

    // EQ commands
    MapEq = 0x12,
    Sw2HwEq = 0x13,
    Hw2SwEq = 0x14,
    QueryEq = 0x15,
    GenEqe = 0x58,

    // CQ commands
    // QP/EE commands
    // special QP and management commands
    // miscellaneous commands
    // Ethernet specific commands
}

pub(super) struct CommandMailBox<'a> {
    hcr: &'a mut Hcr,
    exp_toggle: u32,
}

#[derive(FromBytes)]
#[repr(C, packed)]
struct Hcr {
    in_param: WriteOnly<U64<BigEndian>>,
    in_mod: WriteOnly<U32<BigEndian>>,
    out_param: Volatile<U64<BigEndian>>,
    /// only the first 16 bits are usable
    token: WriteOnly<U32<BigEndian>>,
    /// status includes go, e, t and 5 reserved bits;
    /// opcode includes the opcode modifier
    status_opcode: Volatile<U32<BigEndian>>,
}

impl<'a> CommandMailBox<'a> {
    pub(super) fn new(config_regs: &'a mut MappedPages) -> Result<Self, &'static str> {
        Ok(Self {
            hcr: config_regs.as_type_mut(HCR_BASE)?,
            exp_toggle: 1,
        })
    }

    /// Post a command and wait for its completion.
    /// 
    /// Input and output can be either 0 (for opcodes that take no input or
    /// give no output), *physical* addresses (for opcodes that read from or
    /// write to mailboxes) or integers (for opcodes the operate on immediate
    /// values). Immediate outputs are also returned.
    /// 
    /// ## Safety
    /// 
    /// This function does not check whether addresses are valid or whether
    /// the specified opcode takes the provided type of input or output.
    pub(super) fn execute_command(
        &mut self, opcode: Opcode,
        input: u64, input_modifier: u32, output: u64,
    ) -> Result<u64, ReturnStatus> {
        // TODO: timeout

        // wait until the previous command is done
        while self.is_pending() {}

        // post the command
        trace!("executing command: {opcode:?}");
        self.hcr.in_param.write(input.into());
        self.hcr.in_mod.write(input_modifier.into());
        self.hcr.out_param.write(output.into());
        self.hcr.token.write((POLL_TOKEN << 16).into());
        // TODO: barrier?
        self.hcr.status_opcode.write((
            (1 << HCR_GO_BIT)
            | (self.exp_toggle << HCR_T_BIT)
            | (0 << HCR_E_BIT) // TODO: event
            | (0 << HCR_OPMOD_SHIFT) // TODO: opcode modifier
            | u32::from(opcode)
        ).into());
        self.exp_toggle ^= 1;

        // poll for it
        while self.is_pending() {}

        // check the status
        let status = ReturnStatus::from_repr(
            self.hcr.status_opcode.read().get() >> 24
        ).expect("return status invalid");
        trace!("status: {status:?}");
        match status {
            // on success, return the result
            ReturnStatus::Ok => Ok(self.hcr.out_param.read().get()),
            // else, return the status
            err => Err(err),
        }
    }

    fn is_pending(&self) -> bool {
        let status = self.hcr.status_opcode.read().get();
        trace!("is_pending: got status: {status:#x}");
        status & (1 << HCR_GO_BIT) != 0 || (status & (1 << HCR_T_BIT)) == self.exp_toggle
    }
}

#[repr(u32)]
#[derive(Debug, FromRepr, IntoStaticStr)]
pub(super) enum ReturnStatus {
    // general
    Ok = 0x00,
    InternalErr = 0x01,
    BadOp = 0x02,
    BadParam = 0x03,
    BadSysState = 0x04,
    BadResource = 0x05,
    ResourceBusy = 0x06,
    ExceedLim = 0x08,
    BadResState = 0x09,
    BadIndex = 0x0a,
    BadNvmem = 0x0b,
    IcmError = 0x0c,
    BadPerm = 0x0d,

    // QP state
    BadQpState = 0x10,

    // TPT
    RegBound = 0x21,

    // MAD
    BadPkt = 0x30,

    // CQ
    BadSize = 0x40,
}
