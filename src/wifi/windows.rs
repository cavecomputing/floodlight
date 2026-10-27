//! The networks on Windows, from its wifi service (wlanapi) through windows-sys. netsh can only
//! list what Windows last scanned, and while joined to a network Windows rarely scans, so here
//! each survey asks for a scan and reads what the one before found: nothing waits. Only compiled
//! on Windows; the other OSes list networks through their own tools in survey.rs.

use super::survey::{Network, bond_from_ies};
use std::ptr::{null, null_mut};
use std::slice;
use windows_sys::Win32::NetworkManagement::WiFi::{
    WLAN_BSS_ENTRY, WLAN_BSS_LIST, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
    WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetNetworkBssList, WlanOpenHandle,
    WlanQueryInterface, WlanScan, dot11_BSS_type_any, wlan_intf_opcode_current_connection,
};
use windows_sys::core::GUID;

/// Every access point every wifi card has heard.
pub fn networks() -> Result<Vec<Network>, String> {
    let mut networks = Vec::new();
    // SAFETY: every pointer handed in is a live local or one wlanapi just gave back, every list
    // is read only up to the count wlanapi put in it, and each is freed once after its last use.
    unsafe {
        let mut version = 0;
        let mut handle = null_mut();
        if WlanOpenHandle(2, null(), &mut version, &mut handle) != 0 {
            return Err("the wifi service (WLAN AutoConfig) isn't running".to_string());
        }
        let mut cards: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        if WlanEnumInterfaces(handle, null(), &mut cards) == 0 {
            let count = (*cards).dwNumberOfItems as usize;
            for card in slice::from_raw_parts((*cards).InterfaceInfo.as_ptr(), count) {
                let card = &card.InterfaceGuid;
                // Runs on its own; the list below is what the last scan found.
                WlanScan(handle, card, null(), null(), null());
                let joined = joined_bssid(handle, card);
                let mut heard: *mut WLAN_BSS_LIST = null_mut();
                let status = WlanGetNetworkBssList(
                    handle,
                    card,
                    null(),
                    dot11_BSS_type_any,
                    0,
                    null(),
                    &mut heard,
                );
                if status != 0 {
                    continue;
                }
                let count = (*heard).dwNumberOfItems as usize;
                for entry in slice::from_raw_parts((*heard).wlanBssEntries.as_ptr(), count) {
                    networks.push(network(entry, joined));
                }
                WlanFreeMemory(heard.cast());
            }
            WlanFreeMemory(cards.cast());
        }
        WlanCloseHandle(handle, null());
    }
    if networks.is_empty() {
        // Windows 11 also hands back nothing when the terminal isn't allowed location.
        return Err(
            "no networks heard: is wifi on, and does the terminal have location access?"
                .to_string(),
        );
    }
    Ok(networks)
}

/// One access point as wlanapi heard it. Its width and the centre of its bonded channel come
/// from the operation elements of its beacon, which sit `ulIeOffset` bytes from the start of the
/// entry.
///
/// SAFETY: `entry` must be inside a list wlanapi returned, which keeps its elements after it.
unsafe fn network(entry: &WLAN_BSS_ENTRY, joined: Option<[u8; 6]>) -> Network {
    let ssid = &entry.dot11Ssid;
    let name = String::from_utf8_lossy(&ssid.ucSSID[..(ssid.uSSIDLength as usize).min(32)]);
    let ies = unsafe {
        let start = (entry as *const WLAN_BSS_ENTRY)
            .cast::<u8>()
            .add(entry.ulIeOffset as usize);
        slice::from_raw_parts(start, entry.ulIeSize as usize)
    };
    let freq_mhz = entry.ulChCenterFrequency / 1000;
    let (width_mhz, centre_mhz) = bond_from_ies(ies, freq_mhz);
    Network {
        ssid: (!name.trim_matches('\0').is_empty()).then(|| name.into_owned()),
        bssid: Some(mac(entry.dot11Bssid)),
        freq_mhz,
        width_mhz: Some(width_mhz),
        centre_mhz: Some(centre_mhz),
        signal_dbm: entry.lRssi,
        connected: joined == Some(entry.dot11Bssid),
    }
}

/// The BSSID the card is joined to, if it is.
///
/// SAFETY: `handle` must be open and `card` one of its cards.
unsafe fn joined_bssid(handle: *mut core::ffi::c_void, card: &GUID) -> Option<[u8; 6]> {
    let mut size = 0;
    let mut data = null_mut();
    let status = unsafe {
        WlanQueryInterface(
            handle,
            card,
            wlan_intf_opcode_current_connection,
            null(),
            &mut size,
            &mut data,
            null_mut(),
        )
    };
    if status != 0 {
        return None;
    }
    let connection = data.cast::<WLAN_CONNECTION_ATTRIBUTES>();
    let bssid = unsafe { (*connection).wlanAssociationAttributes.dot11Bssid };
    unsafe { WlanFreeMemory(data) };
    Some(bssid)
}

fn mac(bytes: [u8; 6]) -> String {
    bytes.map(|byte| format!("{byte:02x}")).join(":")
}
