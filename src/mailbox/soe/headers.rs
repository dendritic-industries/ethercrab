#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeOpcode {
    /// Read Request
    ReadRequest = 0x01,
    /// Read Response
    ReadResponse = 0x02,
    /// Write Request
    WriteRequest = 0x03,
    /// Write Response
    WriteResponse = 0x04,
    /// Notification
    Notification = 0x05,
    /// Emergency / Abort
    Emergency = 0x06,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeFragmentationFlag {
    /// Complete Transmission
    CompleteTransmission = 0,
    /// Incomplete Frame
    IncompleteFrame = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeErrorFlag {
    /// No Error
    NoError = 0,
    /// Error Occurred
    ErrorOccurred = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeElementFlag {
    DataStateStatus = 0b00000001,
    NameDescriptor = 0b00000010,
    Attribute = 0b00000100,
    Unit = 0b00001000,
    MinimumValue = 0b00010000,
    MaximumValue = 0b00100000,
    ValueData = 0b01000000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 4)]
pub struct SoeHeader {
    // Byte 0 - opcode and flags
    // Bits 0 thru 2 are the opcode
    #[wire(bits = 3)]
    pub opcode: SoeOpcode,
    // Bit 3 is fragmentation
    #[wire(bits = 1)]
    pub fragmentation: SoeFragmentationFlag,
    // Bit 4 is error
    #[wire(bits = 1)]
    pub error: SoeErrorFlag,
    // Bit 5-7 is drive number
    #[wire(bits = 3)]
    pub drive_num: u8,

    // Byte 1 - element flags
    #[wire(bytes = 1)]
    pub element_flag: SoeElementFlag,

    // Bytes 2 & 3 - IDN
    #[wire(bytes = 2)]
    pub idn: u16,
}

// TODO write tests
// #[cfg(test)]
// mod tests {
//     pub use super::*;
//     use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWriteSized};

//     #[test]
//     fn sanity_soe_service() {
//         assert_eq!(SoeService::SdoRequest.pack(), [0x02]);
//         assert_eq!(
//             SoeService::unpack_from_slice(&[0x02]),
//             Ok(SoeService::SdoRequest)
//         );
//     }
// }
