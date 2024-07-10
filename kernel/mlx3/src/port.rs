use core::{fmt::Debug, mem::size_of};
use memory::MappedPages;
use modular_bitfield_msb::{bitfield, specifiers::{B2, B4, B48, B9}};
use strum_macros::FromRepr;

use crate::cmd::{CommandInterface, MadIfcOpcodeModifier, Opcode, SetPortOpcodeModifier};

#[derive(Debug)]
pub struct Port {
    number: u8,
    capability_mask: u32,
    open: bool,
    capabilities: Option<PortCapabilities>,
}

impl Port {
    pub(super) fn new(
        cmd: &mut CommandInterface, number: u8, mtu: Mtu,
        pkey_table_size: Option<u16>,
    ) -> Result<Self, &'static str> {
        trace!("initializing port {number}...");
        // first, get the caps
        let mut madifc_modifier = MadIfcOpcodeModifier::empty();
        madifc_modifier.insert(MadIfcOpcodeModifier::DISABLE_MKEY_VALIDATION);
        madifc_modifier.insert(MadIfcOpcodeModifier::DISABLE_BKEY_VALIDATION);
        let madifc_input = [
            0x1, 0x1, 0x1, 0x1,
            0x0, 0x0, 0x0, 0x0,
            0x0, 0x0, 0x0, 0x0,
            0x0, 0x0, 0x0, 0x0,
            0x0, 0x15, 0x0, 0x0,
            0x0, 0x0, 0x0, number,
        ];
        let madifc_output: MappedPages = cmd.execute_command(
            Opcode::MadIfc, madifc_modifier, &madifc_input[..], number.into(),
        )?;
        let capability_mask: u32 = *madifc_output.as_type(84)?;

        // set them
        let mut set_port_input = SetPortCommand::new();
        set_port_input.set_capabilities(capability_mask);
        if let Some(size) = pkey_table_size {
            set_port_input.set_change_port_pkey(true);
            set_port_input.set_max_pkey(size);
        }
        set_port_input.set_change_port_mtu(true);
        set_port_input.set_change_port_vl(true);
        set_port_input.set_mtu_cap(mtu as u8);
        for vl_cap_shift in (0..=3).rev() {
            set_port_input.set_vl_cap(1 << vl_cap_shift);
            cmd.execute_command(
                Opcode::SetPort, SetPortOpcodeModifier::IB,
                &set_port_input.bytes[..], number.into(),
            )?;
        }

        // get the current state
        let mut port = Self {
            number, capability_mask, open: false, capabilities: None,
        };
        port.query(cmd)?;

        // finally, bring the port up
        cmd.execute_command(Opcode::InitPort, (), (), number.into())?;
        port.open = true;
        port.query(cmd)?;
        trace!("initialized {port:?}");
        Ok(port)
    }

    pub(super) fn close(
        mut self, cmd: &mut CommandInterface,
    ) -> Result<(), &'static str> {
        cmd.execute_command(Opcode::ClosePort, (), (), self.number.into())?;
        self.open = false;
        Ok(())
    }
    
    /// Query the port capabilities, configuration and current settings.
    fn query(
        &mut self, cmd: &mut CommandInterface,
    ) -> Result<(), &'static str> {
        let page: MappedPages = cmd.execute_command(
            Opcode::QueryPort, (), (), self.number.into(),
        )?;
        self.capabilities = Some(PortCapabilities::from_bytes(
            page
                .as_slice(0, size_of::<PortCapabilities>())?
                .try_into()
                .unwrap()
        ));
        Ok(())
    }
    
    /// Get statistics about this port.
    /// 
    /// This is a bit similar to libibumad's `get_port`.
    pub(crate) fn get_stats(
        &mut self, cmd: &mut CommandInterface,
    ) -> Result<PortStats, &'static str> {
        self.query(cmd)?;
        Ok(PortStats {
            number: self.number,
            link_up: self.capabilities.as_ref().unwrap().link_up(),
            capability_mask: self.capability_mask,
            layer: PortStatsLayer::Infiniband,
        })
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        if self.open {
            panic!("Please close instead of dropping")
        }
    }
}

#[repr(u8)]
#[derive(Debug, FromRepr)]
pub(super) enum Mtu {
    Mtu256 = 1,
    Mtu512 = 2,
    Mtu1024 = 3,
    Mtu2048 = 4,
    Mtu4096 = 5,
}

#[bitfield]
struct SetPortCommand {
    #[skip] __: B9,
    change_port_mtu: bool,
    change_port_vl: bool,
    change_port_pkey: bool,
    #[skip] __: B4,
    mtu_cap: B4,
    #[skip] __: B4,
    vl_cap: B4,
    #[skip] __: B4,
    capabilities: u32,
    #[skip] __: u64,
    #[skip] __: u64,
    #[skip] __: u64,
    #[skip] __: u32,
    #[skip] __: u32,
    max_pkey: u16,
    // ...
}

#[bitfield]
struct PortCapabilities {
    link_up: bool,
    // dmfs_optimized_state
    #[skip] __: B2,
    default_sense: bool,
    default_type: bool,
    #[skip] __: bool,
    eth: bool,
    ib: bool,
    #[skip] __: B4,
    ib_mtu: B4,
    eth_mtu: u16,
    ib_link_speed: u8,
    eth_link_speed: u8,
    ib_port_width: u8,
    log_max_gids: B4,
    log_max_pkeys: B4,
    #[skip] __: u16,
    log_max_vlan: B4,
    log_max_mac: B4,
    max_tc_eth: B4,
    max_vl_ib: B4,
    #[skip] __: B48,
    mac: B48,
    // ...
}

impl Debug for PortCapabilities {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f
            .debug_struct("PortCapabilities")
            .field("IB supported", &self.ib())
            .field("Ethernet supported", &self.eth())
            .field("Link", &self.link_up())
            .field("IB MTU", &Mtu::from_repr(self.ib_mtu()))
            .field("Eth MTU", &self.eth_mtu())
            .field("Port MAC", &self.mac())
            .finish()
    }
}

/// Statistics about the port
/// 
/// This is a bit similar to libibumad's `umad_port_t`.
pub struct PortStats {
    pub number: u8,
    pub link_up: bool,
    pub capability_mask: u32,
    pub layer: PortStatsLayer,
}

#[derive(Debug)]
pub enum PortStatsLayer { Infiniband, Ethernet }
