//! This module consists of functions to create a direct memory access mailbox for passing parameters to the hca
//! and getting output back from the hca during verb calls and functions to execute verb calls.

use byteorder::BigEndian;
use memory::{MappedPages, PhysicalAddress};
use num_enum_derive::IntoPrimitive;
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
    UnmapFa = 0xffe,
    MapFa = 0xfff,
    RunFw = 0xff6,
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
    MapIcm = 0xffa,
    MapIcmAux = 0xffc,
    SetIcmSize = 0xffd,

    // TPT commands
    // EQ commands
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
    out_param: WriteOnly<U64<BigEndian>>,
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
    /// Any addresses referenced here are *physical* ones,
    /// because the card has to work with them.
    pub(super) fn execute_command(
        &mut self, opcode: Opcode,
        input: Option<PhysicalAddress>, input_modifier: u32,
        output: Option<PhysicalAddress>,
    ) -> Result<(), &'static str> {
        // TODO: timeout

        // wait until the previous command is done
        while self.is_pending() {}

        // post the command
        trace!("executing command: {opcode:?}");
        self.hcr.in_param.write(
            input.map_or(0, |i| i.value() as u64).into()
        );
        self.hcr.in_mod.write(input_modifier.into());
        self.hcr.out_param.write(
            output.map_or(0, |o| o.value() as u64).into()
        );
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

        // read the result
        // TODO: read the actual result
        match self.hcr.status_opcode.read().get() >> 24 {
            0 => Ok(()),
            // TODO: interpret the number
            _ => Err("Status failed with error"),
        }
    }

    fn is_pending(&self) -> bool {
        let status = self.hcr.status_opcode.read().get();
        trace!("is_pending: got status: {status:#x}");
        status & (1 << HCR_GO_BIT) != 0 || (status & (1 << HCR_T_BIT)) == self.exp_toggle
    }
}


