use ethercrab_wire::EtherCrabWireReadWrite;

use super::SoeHeader;
use crate::mailbox::{
    MailboxHeader, MailboxType, Priority,
    soe::{
        SoeOpcode,
        headers::{SoeElementFlag, SoeErrorFlag, SoeFragmentationFlag},
    },
};

#[derive(Debug, Copy, Clone, PartialEq, EtherCrabWireReadWrite)]
#[wire(bytes = 10)]
pub struct IdnHeader {
    #[wire(bytes = 6)]
    pub mailbox_header: MailboxHeader,
    #[wire(bytes = 4)]
    pub soe_header: SoeHeader,
}

impl IdnHeader {
    pub fn for_reading(
        counter: u8,
        drive_num: u8,
        idn_address: u16,
        flag: SoeElementFlag,
    ) -> IdnHeader {
        let mailbox_header = MailboxHeader {
            length: 0x04, // Only SoE header, no payload
            // address: 0x0000,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Soe,
            counter,
        };

        let soe_header = SoeHeader {
            opcode: SoeOpcode::ReadRequest,
            fragmentation: SoeFragmentationFlag::CompleteTransmission,
            error: SoeErrorFlag::NoError,
            drive_num,
            element_flag: flag,
            idn: idn_address,
        };

        IdnHeader {
            mailbox_header,
            soe_header,
        }
    }

    pub fn for_writing(
        counter: u8,
        drive_num: u8,
        idn_address: u16,
        flag: SoeElementFlag,
        data_len: u16,
    ) -> IdnHeader {
        let mailbox_header = MailboxHeader {
            length: 0x04 + data_len, // Only SoE header + payload
            // address: 0x0000,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Soe,
            counter,
        };

        let soe_header = SoeHeader {
            opcode: SoeOpcode::WriteRequest,
            fragmentation: SoeFragmentationFlag::CompleteTransmission,
            error: SoeErrorFlag::NoError,
            drive_num,
            element_flag: flag,
            idn: idn_address,
        };

        IdnHeader {
            mailbox_header,
            soe_header,
        }
    }
}
