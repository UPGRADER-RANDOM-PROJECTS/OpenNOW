import QtQuick
import OpenNOW

Column {
    id: page
    objectName: "desktopNetworkSettings"
    required property real availableWidth
    required property var settingsScreen
    property var awdlController: MacAwdl

    width: page.availableWidth; spacing: DesktopTokens.px(12)
    Component.onCompleted: ShellStore.refreshRegions()
    DesktopSettingsPanel {
        width: parent.width; paperStyle: true
        DesktopSettingsSection { text: qsTr("CONNECTION") }
        DesktopSettingsChoice {
            objectName: "renewNetworkRegion"
            width: parent.width; glyph: "globe"; title: qsTr("Server region")
            description: ShellStore.regions.length ? qsTr("%1 streaming regions from your account").arg(ShellStore.regions.length) : qsTr("Sign in to discover available regions")
            maximumColumns: 4
            maximumOptionsHeight: DesktopTokens.px(420)
            filterPlaceholder: qsTr("Search regions…")
            items: page.settingsScreen.regionChoiceItems()
            value: {
                const selected = String(page.settingsScreen.valueSetting("region",""))
                const region = ShellStore.regions.find(item => item.url === selected || item.name === selected)
                return region ? region.url : selected
            }
            valueLabel: page.settingsScreen.currentRegionLabel()
            onSelected: value => page.settingsScreen.setSetting("region",value)
        }
        DesktopSettingsRow {
            objectName: "renewRegionLatency"
            width: parent.width; paperStyle: true; glyph: "speed"; title: qsTr("Region latency")
            showDivider: false
            description: ShellStore.regionPingMessage || qsTr("Measure available regions before your next session")
            value: ShellStore.regionPingBusy || page.settingsScreen.currentRegionPing() === null ? ""
                : page.settingsScreen.valueSetting("region", "") === "" ? qsTr("Best: %1 ms").arg(page.settingsScreen.currentRegionPing())
                : page.settingsScreen.currentRegionPing() + " ms"
            DesktopSettingsButton {
                objectName: "renewRegionPingButton"
                text: ShellStore.regionPingPending ? qsTr("Loading…") : ShellStore.regionPingBusy ? qsTr("Pinging…") : qsTr("Ping regions")
                enabled: !ShellStore.regionPingBusy
                onClicked: ShellStore.pingRegions()
            }
        }
        DesktopSettingsRow {
            width: parent.width; paperStyle: true; glyph: "globe"; title: qsTr("Auto-select best region")
            description: qsTr("Choose the closest streaming region with the lowest ping before each session. Useful when GeForce NOW is not available in your country.")
            showDivider: false
            DesktopSettingsToggle {
                objectName: "autoSelectRegionToggle"
                checked: ShellStore.autoSelectRegion
                enabled: !ShellStore.selectingBestRegion
                Accessible.name: qsTr("Auto-select best region")
                onValueChangedByUser: value => ShellStore.autoSelectRegion = value
            }
        }
        DesktopSettingsRow {
            visible: ShellStore.selectingBestRegion
            width: parent.width; paperStyle: true; glyph: "speed"; title: qsTr("Selecting best region…")
            description: qsTr("Measuring all regions and picking the one with the lowest ping.")
            showDivider: false
        }
        DesktopSettingsRow {
            visible: ShellStore.queueSelectorFreeTier
            width: parent.width; paperStyle: true; glyph: "globe"
            title: qsTr("Free-tier queue selector")
            description: qsTr("Compare queues and latency before launching a game")
            showDivider: false
            DesktopSettingsToggle {
                objectName: "queueSelectorEnabled"
                checked: !page.settingsScreen.boolSetting("hideQueueSelector", false)
                onValueChangedByUser: value => page.settingsScreen.setSetting("hideQueueSelector", !value)
            }
        }
    }
    DesktopSettingsPanel {
        visible: page.awdlController.state !== MacAwdlController.Unsupported
        width: parent.width; paperStyle: true
        DesktopSettingsSection { text: qsTr("MACOS WI-FI") }
        DesktopSettingsRow {
            objectName: "suppressAwdlWhileStreamingRow"
            width: parent.width; paperStyle: true; glyph: "globe"
            title: qsTr("Suppress AWDL while streaming")
            description: qsTr("May reduce Wi-Fi latency spikes. Temporarily interrupts AirDrop, AirPlay, Sidecar, Universal Control and other Continuity features for all users while a stream is active. AWDL is restored when the stream ends or OpenNOW disconnects.")
            DesktopSettingsToggle {
                objectName: "suppressAwdlWhileStreamingToggle"
                enabled: !page.awdlController.busy
                    && (page.awdlController.state === MacAwdlController.Installed || checked)
                checked: page.settingsScreen.boolSetting("suppressAwdlWhileStreaming", false)
                onValueChangedByUser: value => page.settingsScreen.setSetting("suppressAwdlWhileStreaming", value)
            }
        }
        DesktopSettingsRow {
            objectName: "awdlHelperStatus"
            width: parent.width; paperStyle: true; glyph: "shield"
            title: qsTr("AWDL helper")
            description: page.awdlController.busy ? qsTr("Waiting for macOS authorization…")
                : page.awdlController.error !== "" ? qsTr("Error")
                : page.awdlController.leased ? qsTr("Active")
                : page.awdlController.state === MacAwdlController.Installed ? qsTr("Ready")
                : qsTr("Not installed")
            DesktopSettingsButton {
                objectName: "awdlHelperInstallButton"
                enabled: !page.awdlController.busy
                text: page.awdlController.state === MacAwdlController.Installed ? qsTr("Remove helper…") : qsTr("Install helper…")
                onClicked: {
                    if (page.awdlController.state === MacAwdlController.Installed)
                        page.awdlController.uninstall()
                    else
                        page.awdlController.install()
                }
            }
            DesktopSettingsButton {
                objectName: "awdlHelperRefreshButton"
                text: qsTr("Refresh")
                enabled: !page.awdlController.busy
                onClicked: page.awdlController.refresh()
            }
        }
        DesktopSettingsRow {
            width: parent.width; paperStyle: true; glyph: "info"
            title: qsTr("Administrator authorization")
            description: qsTr("Installing or removing the helper requires administrator approval once. Only install it if you trust this copy of OpenNOW. Ad-hoc signing does not verify the publisher.")
            showDivider: false
        }
        DesktopSettingsRow {
            objectName: "awdlHelperError"
            visible: page.awdlController.error !== ""
            width: parent.width; paperStyle: true; glyph: "info"
            title: qsTr("AWDL helper needs attention")
            description: page.awdlController.error
            showDivider: false
        }
    }
    DesktopSettingsAdvanced {
        detail: qsTr("Transport · Network test · API proxy")
        expanded: page.settingsScreen.advancedOpen; onClicked: page.settingsScreen.advancedOpen = !page.settingsScreen.advancedOpen
    }
    DesktopSettingsDisclosure {
        objectName: "networkAdvancedDisclosure"
        width: parent.width; expanded: page.settingsScreen.advancedOpen
        sourceComponent: DesktopSettingsPanel {
            width: page.availableWidth; paperStyle: true
            DesktopSettingsSection { text: qsTr("TRANSPORT") }
            DesktopSettingsChoice {
                objectName: "webrtcCompatibilityModeChoice"
                width: parent.width; glyph: "globe"
                title: qsTr("WebRTC compatibility mode")
                description: qsTr("Automatic uses WebRTC for signed-in alliance accounts and NVST for NVIDIA accounts. Choose WebRTC or NVST to override either account type. Applies to new sessions only; existing sessions keep their transport.")
                    + "\n" + qsTr("WebRTC is for compatibility only. Expect lower performance than NVST. Supports H.264 and H.265, SDR, and stereo. H.265 supports 10-bit color and the selected resolution and frame rate. No microphone or clipboard text. Requires direct UDP connectivity.")
                enabled: !ShellStore.activeSession && !ShellStore.streamBusy && !ShellStore.pendingLaunchParams
                maximumColumns: 3
                items: [{value:"auto", label:qsTr("Automatic")},
                    {value:"on", label:qsTr("WebRTC")}, {value:"off", label:qsTr("NVST")}]
                value: page.settingsScreen.valueSetting("webrtcCompatibilityMode", "auto")
                onSelected: value => page.settingsScreen.setSetting("webrtcCompatibilityMode", value)
            }
            DesktopSettingsRow {
                width: parent.width; paperStyle: true; glyph: "bolt"; title: qsTr("L4S")
                description: qsTr("Request scalable low-latency transport for the next session")
                DesktopSettingsToggle { checked: page.settingsScreen.boolSetting("enableL4S",false); onValueChangedByUser: value => page.settingsScreen.setSetting("enableL4S",value) }
            }
            DesktopSettingsRow {
                objectName: "renewNetworkTest"
                width: parent.width; paperStyle: true; glyph: "speed"; title: qsTr("Network test")
                showDivider: false
                description: qsTr("Measure this zone's UDP payload reachability before streaming · selected zones only")
                DesktopSettingsToggle { objectName: "renewNetworkTestToggle"; checked: page.settingsScreen.boolSetting("networkTest",false); onValueChangedByUser: value => page.settingsScreen.setSetting("networkTest",value) }
            }
            DesktopSettingsSection { text: qsTr("API PROXY") }
            DesktopSettingsRow {
                width: parent.width; paperStyle: true; glyph: "globe"; title: qsTr("Use proxy")
                description: qsTr("Applies to API calls only · the stream always goes direct")
                DesktopSettingsToggle { objectName: "renewProxyEnabled"; checked: page.settingsScreen.boolSetting("sessionProxyEnabled",false); onValueChangedByUser: value => page.settingsScreen.setSetting("sessionProxyEnabled",value) }
            }
            DesktopSettingsRow {
                visible: page.settingsScreen.boolSetting("sessionProxyEnabled", false)
                width: parent.width; paperStyle: true; glyph: "arrows"; title: qsTr("Proxy address")
                description: qsTr("Leave empty to use a direct connection"); showDivider: false
                DesktopSettingsField {
                    objectName: "renewProxyAddress"
                    width: DesktopTokens.px(300)
                    text: String(page.settingsScreen.valueSetting("sessionProxyUrl",""))
                    placeholderText: qsTr("http://proxy.example:8080")
                    Accessible.name: qsTr("Proxy address")
                    onEditingFinished: page.settingsScreen.setSetting("sessionProxyUrl",text)
                }
            }
        }
    }
}
