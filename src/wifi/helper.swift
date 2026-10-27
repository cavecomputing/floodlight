// The macOS WiFi helper: lists the networks in earshot, one per line, into the file named by its
// first argument. It is a small app rather than a command because macOS shows network names only
// to an app the user has allowed Location, and a terminal tool can't ask for that. build.rs
// compiles it; src/wifi/survey.rs wraps it in an app bundle and reads what it writes.
//
// Each line: ssid, bssid, channel, band (2, 5 or 6), width in MHz (0 if unknown), signal in dBm,
// `joined` or nothing, separated by tabs. Or a single line `denied` when Location isn't allowed,
// or `error`, a tab and why.

import AppKit
import CoreLocation
import CoreWLAN

let listPath = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "/dev/stdout"

func finish(_ text: String) -> Never {
    try? text.write(toFile: listPath, atomically: true, encoding: .utf8)
    exit(0)
}

/// A field with the tabs and line breaks that would split the line taken out.
func field(_ text: String) -> String {
    text.replacingOccurrences(of: "\t", with: " ").replacingOccurrences(of: "\n", with: " ")
}

func scan() -> Never {
    guard let wifi = CWWiFiClient.shared().interface() else {
        finish("error\tno wifi card found\n")
    }
    let networks: Set<CWNetwork>
    do {
        networks = try wifi.scanForNetworks(withSSID: nil)
    } catch {
        finish("error\t\(field(error.localizedDescription))\n")
    }
    let joined = wifi.bssid()
    var lines = ""
    for network in networks {
        guard let channel = network.wlanChannel else { continue }
        // The raw values of CWChannelBand and CWChannelWidth, which are stable across SDKs.
        let band = [1: 2, 2: 5, 3: 6][channel.channelBand.rawValue] ?? 0
        let width = [1: 20, 2: 40, 3: 80, 4: 160][channel.channelWidth.rawValue] ?? 0
        let bssid = network.bssid ?? ""
        let fields = [
            field(network.ssid ?? ""), bssid, "\(channel.channelNumber)", "\(band)", "\(width)",
            "\(network.rssiValue)", !bssid.isEmpty && bssid == joined ? "joined" : "",
        ]
        lines += fields.joined(separator: "\t") + "\n"
    }
    finish(lines)
}

/// Scans once Location is allowed, asks if nobody has decided yet, gives up if it's refused.
class Permission: NSObject, CLLocationManagerDelegate {
    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        switch manager.authorizationStatus {
        case .notDetermined: manager.requestWhenInUseAuthorization()
        case .denied, .restricted: finish("denied\n")
        default: scan()
        }
    }
}

let location = CLLocationManager()
let permission = Permission()
location.delegate = permission
// Nobody answered the prompt: say so rather than hang the terminal.
DispatchQueue.main.asyncAfter(deadline: .now() + 60) { finish("denied\n") }
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
app.run()
