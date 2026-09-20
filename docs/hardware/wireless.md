# Wi-Fi over the wireless class

A radio is read from its phy under `/sys/class/ieee80211`: the `device`
link to the PCI function it is driven on, whose `vendor` and `device` are
world-readable, and `macaddress` in the phy's own directory. The read is
`hardware/src/wireless.rs` over `hardware/src/pci.rs`; the names for the
PCI ids come from udev's cached record, `hardware/src/udev.rs`.

Each section holds what the kernel, the drivers or this code establishes,
then under **Observed** what was read. The Laptop 13 Pro (Intel Core Ultra
Series 3) was read on the machine; other boards were read from published
probe logs, and each row names the board.

## Contents

Every heading in the file appears here.

- [What the class carries](#what-the-class-carries)
- [Chipset radio or card](#chipset-radio-or-card)
- [What is not in sysfs](#what-is-not-in-sysfs)
- [Radios by board, observed](#radios-by-board-observed)
- [Open](#open)
- [Sources](#sources)

## What the class carries

| Path | Reads |
|---|---|
| `phy*/device` | the PCI function; `vendor` and `device` under it |
| `phy*/macaddress` | the address the radio answers to |
| `phy*/addresses`, `index`, `name` | the phy's own bookkeeping; no capability data |

- The class carries no name for the module. A name comes from udev's PCI
  database or from nowhere.

## Chipset radio or card

| Wiring | What the PCI ids name | Boards |
|---|---|---|
| Intel CNVi: the MAC is in the SoC, the M.2 module is an RF companion, the function sits on bus 0 | the platform; every machine of a generation reads alike whichever module is in the slot | the Intel Laptop 13 boards |
| A card on its own bus | the module | the AMD boards, with MediaTek cards |

- `iwlwifi` names a CNVi module from an RF id it reads over the interface
  and logs the name. Neither the id nor the name reaches sysfs.
- Consequence: a catalogue keyed on PCI ids can name a discrete card and
  not a CNVi module. `model::part` carries arms for the MediaTek ids and
  the AX210 and none for a CNVi function.
- Bluetooth on a CNVi board is the same silicon on a neighboring function,
  with no USB companion on the bus to name the module either.

## What is not in sysfs

| Fact | How it is answered |
|---|---|
| The running firmware version | `ethtool -i`, an ioctl the driver answers; changes with every driver update |
| Bands and channel width | `NL80211_CMD_GET_WIPHY` over generic netlink |

- Consequence: the Wi-Fi part carries no firmware, and its bands would
  need an nl80211 client. The bands and width are what separates two
  modules of one CNVi generation, a BE211 from a BE213 among them.

## Radios by board, observed

| Board | Function | Ids | Subsystem | Driver | udev name |
|---|---|---|---|---|---|
| Laptop 13 Pro | `0000:00:14.3`, bus 0 | `8086:e440` | `8086:0114` | `iwlwifi` | none for `e440`; vendor only |
| Laptop 13, 11th to 13th Gen Intel Core | `0000:00:14.3` | `8086:a0f0`, `8086:51f0` | | `iwlwifi` | |
| Intel Laptop 13 boards with the AX210 card | own bus | `8086:2725` | `8086:0024` or `8086:0020` between units | `iwlwifi` | named |
| AMD boards with the RZ616 | own bus | `14c3:0616` | | | named |
| AMD boards with the RZ717 | own bus | `14c3:0717` | | | named |

- On the Laptop 13 Pro `iwlwifi` logs the module as a BE211 from the RF id
  it read; `NL80211_CMD_GET_WIPHY` lists bands 1, 2 and 4, a 160 MHz
  channel width on the 5 GHz band and 320 MHz in the 6 GHz band's EHT
  capabilities.
- Its Bluetooth is `0000:00:14.7`, `8086:e476`, driver `btintel_pcie`.

## Open

- What the CNVi subsystem id `8086:0114` denotes, which needs a second
  Intel machine to compare against.
- Which AX210 subsystem id goes with which module revision.

## Sources

- The Linux kernel's `ieee80211` class and `iwlwifi` driver, and the
  nl80211 interface in `include/uapi/linux/nl80211.h`.
- udev's `/run/udev/data` records and `hwdb.bin`, for the names behind PCI
  ids.
- Probe logs of Framework laptops published at linux-hardware.org — the ids
  of every board other than the Laptop 13 Pro.
