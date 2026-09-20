# DMI

What the project reads from the firmware's SMBIOS table, and how.

The table is reached two ways, with different permissions. The kernel
publishes a handful of fields as world-readable files under
`/sys/class/dmi/id`; the board is identified from those. It keeps the raw
structures under `/sys/firmware/dmi/entries` root-only; the memory modules
are read from those. An unprivileged process can therefore identify the board
but sees no memory at all.

The reads are `hardware/src/dmi.rs` and the board mapping is
`hardware/src/platform.rs`. [`parts.md`](parts.md) lists which parts come over
this transport.

## Contents

Every heading in the file appears here.

- [Board detection](#board-detection)
  - [The vendor match](#the-vendor-match)
  - [The board names](#the-board-names)
    - [Laptop 13](#laptop-13)
    - [Laptop 12](#laptop-12)
    - [Laptop 16](#laptop-16)
    - [Desktop](#desktop)
  - [The BIOS build date](#the-bios-build-date)
- [Memory detection](#memory-detection)
  - [Reading a structure](#reading-a-structure)
  - [The fields read](#the-fields-read)
  - [Size](#size)
  - [Speed](#speed)
  - [Values not listed in the specification](#values-not-listed-in-the-specification)
  - [Padded strings](#padded-strings)
  - [No manufacturing date](#no-manufacturing-date)
- [Sources](#sources)

## Board detection

These fields are read from `/sys/class/dmi/id`, each trimmed of the newline
sysfs appends:

| Field | Used for |
|---|---|
| `sys_vendor` | the vendor match below |
| `product_name` | the machine the board is sold for |
| `board_vendor` | the mainboard part's vendor |
| `board_name` | the mainboard part's number |
| `board_serial` | the mainboard part's serial; root-only |
| `product_sku` | reported as the mainboard's SKU, and nothing more |
| `bios_version` | the BIOS firmware version |
| `bios_date` | when the BIOS was built |

A field the kernel withholds from the process, and a field the firmware left
out, both read as absent.

### The vendor match

`sys_vendor` must equal `Framework` exactly. Nothing treats the machine as
Framework hardware unless it does, and no product name is matched before the
vendor is.

The vendor gate is not a formality. `product_name` on the 11th Gen Intel
Laptop 13 is the bare string `Laptop`, which another manufacturer can ship
just as easily, so a product name on its own cannot identify the hardware.

### The board names

`product_name` is matched whole. These are the strings the
firmware reports, in its own spelling, and they are the same strings
`framework_lib` matches in its own `get_platform` — see [Sources](#sources).

They are matched here and nowhere else. What leaves `hardware` is a
`Platform`, the enum both binaries spell: a string one binary matches stays
in that binary, and what crosses the bus is the vocabulary the two ends must
agree on. The strings still cross for a reader — the vendor on the empty
page, the product as the mainboard's model — but nothing outside `hardware`
matches one.

#### Laptop 13

| `product_name` | Variant |
|---|---|
| `Laptop` | 11th Gen Intel Core |
| `Laptop (12th Gen Intel Core)` | 12th Gen Intel Core |
| `Laptop (13th Gen Intel Core)` | 13th Gen Intel Core |
| `Laptop 13 (Intel Core Ultra Series 1)` | Intel Core Ultra Series 1 |
| `Laptop 13 (AMD Ryzen 7040 Series)` | AMD Ryzen 7040 Series |
| `Laptop 13 (AMD Ryzen 7040Series)` | AMD Ryzen 7040 Series, on firmware that omits the space |
| `Laptop 13 (AMD Ryzen AI 300 Series)` | AMD Ryzen AI 300 Series |
| `Laptop 13 Pro (Intel Core Ultra Series 3)` | Pro, Intel Core Ultra Series 3 |

The first three report a product name without the 13 in it, and the Pro is a
chassis variant of the 13 rather than a series of its own.

#### Laptop 12

| `product_name` | Variant |
|---|---|
| `Laptop 12 (13th Gen Intel Core)` | 13th Gen Intel Core |
| `Laptop 12 (Intel Core Series 3)` | Intel Core Series 3 |

#### Laptop 16

| `product_name` | Variant |
|---|---|
| `Laptop 16 (AMD Ryzen 7040 Series)` | AMD Ryzen 7040 Series |
| `Laptop 16 (AMD Ryzen AI 300 Series)` | AMD Ryzen AI 300 Series |

#### Desktop

| `product_name` | Variant |
|---|---|
| `Desktop (AMD Ryzen AI Max 300 Series)` | AMD Ryzen AI Max 300 Series |

A variant stops at the processor generation. Where a board is sold in more
than one processor configuration, the name does not distinguish them, and the
DMI table carries nothing that does.

A board reports a second identifier beside its part number: `product_sku`
reads `FRANVXCP07` on a machine whose `board_name` reads `FRANMJCP07`, and
Framework's marketplace carries a third family in its variant codes. The
resemblance invites the conclusion that the three are one scheme, which would
make a board number a route to the exact listing and to the processor.
Nothing confirms how they relate, so each is reported as it stands.

One place matches a product name — the mapping above — and three tables key
on the `Platform` it answers, each needing a different fact about the board:

- The mainboard's catalogue entry, for the name the board is sold under and
  its marketplace link.
- The USB-C port layout, for where each port's socket is on the chassis.
- The touchscreen's processor pad, which is known for one board only and
  refuses to drive an unknown line on any other.

### The BIOS build date

The specification asks firmware for `mm/dd/yyyy`. A stamp of that shape is
converted to an ISO date; anything else is shown as the firmware wrote it. A
two-digit year is not converted, because which century it means is a guess and
the stamp as written is more useful than a wrong date.

## Memory detection

Modules come from SMBIOS type 17, "Memory Device", one structure per slot,
read from `/sys/firmware/dmi/entries/17-<instance>/raw`. Instances are read in
order until a path is missing. The path is root-only, so an unprivileged
process finds nothing here.

Every field, encoding and table cited below is from section 7.18, "Memory
Device (Type 17)", of the SMBIOS specification — see [Sources](#sources).

### Reading a structure

A structure is a formatted area followed by a string table. Byte 1 of the
header gives the formatted area's length, and the string table starts after
it. A byte in the formatted area that refers to a string holds a one-based index
into that table; an index of zero is the specification's "no string". Bytes
shorter than the header's stated length are not a structure and are skipped.

### The fields read

| Offset | Field | Read as |
|---|---|---|
| `0x0c` | Size | u16 |
| `0x0e` | Form factor | byte, table 76 |
| `0x10` | Device locator | string |
| `0x12` | Memory type | byte, table 77 |
| `0x15` | Speed | u16 |
| `0x17` | Manufacturer | string |
| `0x18` | Serial number | string |
| `0x1a` | Part number | string |
| `0x1c` | Extended size | u32 |
| `0x20` | Configured speed | u16 |
| `0x54` | Extended speed | u32 |
| `0x58` | Extended configured speed | u32 |

The device locator becomes the part's id, as `dmi-slot:<locator>`.

### Size

The field counts megabytes up to 32766. For anything larger, firmware writes
`0x7fff` and puts the figure in the 32-bit extension at `0x1c`, also in
megabytes with its top bit reserved — so a 32 GB module arrives there.

`0` and `0xffff` mean no module, and so does `0x7fff` with a zero extension.
Empty slots are listed by the table and left out of the parts: a slot is a
fact about the board, not a part.

### Speed

Both speed fields use the same encoding. `0` means the rate is unavailable.
`0xffff` defers to the 32-bit extension beside it. Anything else is the rate
in MT/s.

The configured speed is reported only where it differs from the rated speed.
A platform clocking a module below its rating is worth showing; two rows of
the same number are not: the Laptop 13 Pro reports a module rated 8533 MT/s
and configured at 7467.

### Values not listed in the specification

Form factor and memory type are byte values spelled from the specification's
tables 76, "Memory Device: Form Factor field", and 77, "Memory Device: Type".

A value neither table lists is shown as hexadecimal rather than as "unknown":
firmware outruns whichever revision the tables here were written against, and
the raw byte is more use than a word that hides it.

### Padded strings

Firmware pads a string field to its own width, so a value shorter than the
field arrives with trailing blanks. Every string is trimmed as the structure
is parsed, since a value matched or shown untrimmed carries the padding with
it.

### No manufacturing date

Type 17 has no such field, and the week and year the module's own SPD carries
are not exposed.

## Sources

- [`framework_lib::smbios::get_platform`](https://github.com/FrameworkComputer/framework-system/blob/main/framework_lib/src/smbios.rs)
  — the match from `product_name` to a platform, and the source of every
  string in the tables above, the unspaced `Laptop 13 (AMD Ryzen 7040Series)`
  included.
- [DMTF **DSP0134**, *System Management BIOS (SMBIOS) Reference
  Specification*, version **3.10.0**](https://www.dmtf.org/dsp/DSP0134) —
  section **7.18, "Memory Device (Type 17)"** carries the field offsets, the
  size and speed encodings, and tables 76 and 77. That revision lists form
  factors through `13h`, `CSODIMM`, and memory types through `26h`, `LPDDR6`.
  The structure format and the string table's one-based indexing are the
  specification's general clauses rather than 7.18's.
- The Linux kernel's DMI sysfs interface — `/sys/class/dmi/id` for the fields
  the kernel publishes world-readable, and `/sys/firmware/dmi/entries` for the
  raw structures it keeps root-only, documented under
  `Documentation/ABI/testing/sysfs-firmware-dmi-entries`.
