"""Regenerate the synthetic regression workbooks used by tests/regression.rs.

    pip install openpyxl
    python3 testdata/generate_testdata.py

Capacities are in MiB (1 TiB = 1048576 MiB).

Baseline (no vDisk tab) — powered-on only:
  vm1  DC-A/CL1  vInfo 1 TiB,   vPartition 0.25+0.25 TiB -> 0.5 TiB
  vm2  DC-A/CL1  vInfo 0.5 TiB, vPartition 1 TiB         -> 0.5 TiB
  vm3  DC-A/CL2  vInfo 2 TiB,   no vPartition rows       -> 2.0 TiB
  vm4  DC-B/CL3  vInfo 1 TiB,   vPartition 0.75 TiB      -> 0.75 TiB
  vm5  DC-B/CL3  poweredOff, vInfo 1 TiB, vPartition 0.5 TiB (only with -i)
Total 3.75 TB / 4 VMs (4.25 TB / 5 VMs with -i; 4.5 TB with -d).

Disk-awareness workbook (regression_disks.xlsx):
  vm_vmdk   — one thin VMDK 1 TiB, guest 0.5 TiB -> 0.5 (unchanged by disk awareness)
  vm_prdm   — VMDK 0.25 TiB + pRDM 2 TiB; guest 2.0 TiB; vInfo 2.25
              default: eligible 0.25, cap guest -> 0.25; --keep-rdm: min(2.25, 2.0)=2.0
  vm_vrdm   — virtualMode RDM 1 TiB, guest 0.8, vInfo 1.0 -> 0.8 (kept)
  vm_cap    — VMDK 0.5 TiB, guest 2.0 (inflated), vInfo 2.0 -> capped to 0.5
  vm_only_p — only physical RDM 1 TiB; excluded by default; with --keep-rdm: min(1,1)=1
"""
from pathlib import Path
from openpyxl import Workbook

T = 1048576
VINFO = [
    ("vm1", "poweredOn", "DC-A", "CL1", T),
    ("vm2", "poweredOn", "DC-A", "CL1", T // 2),
    ("vm3", "poweredOn", "DC-A", "CL2", 2 * T),
    ("vm4", "poweredOn", "DC-B", "CL3", T),
    ("vm5", "poweredOff", "DC-B", "CL3", T),
]
VPART = [
    ("vm1", "poweredOn", T // 4),
    ("vm1", "poweredOn", T // 4),
    ("vm2", "poweredOn", T),
    ("vm4", "poweredOn", 3 * T // 4),
    ("vm5", "poweredOff", T // 2),
]


def build_baseline(path, part_powerstate=True):
    wb = Workbook()
    ws = wb.active
    ws.title = "vInfo"
    ws.append(["VM", "Powerstate", "Datacenter", "Cluster", "In Use MiB"])
    for row in VINFO:
        ws.append(list(row))
    p = wb.create_sheet("vPartition")
    if part_powerstate:
        p.append(["VM", "Powerstate", "Consumed MiB"])
        for row in VPART:
            p.append(list(row))
    else:
        p.append(["VM", "Consumed MiB"])
        for vm, _, cap in VPART:
            p.append([vm, cap])
    wb.save(path)


def build_disks(path):
    wb = Workbook()
    info = wb.active
    info.title = "vInfo"
    info.append(["VM", "Powerstate", "Datacenter", "Cluster", "In Use MiB"])
    rows = [
        ("vm_vmdk", "poweredOn", "DC-D", "CL1", T),
        ("vm_prdm", "poweredOn", "DC-D", "CL1", 9 * T / 4),
        ("vm_vrdm", "poweredOn", "DC-D", "CL2", T),
        ("vm_cap", "poweredOn", "DC-D", "CL2", 2 * T),
        ("vm_only_p", "poweredOn", "DC-D", "CL3", T),
    ]
    for r in rows:
        info.append(list(r))

    part = wb.create_sheet("vPartition")
    part.append(["VM", "Powerstate", "Consumed MiB"])
    for r in [
        ("vm_vmdk", "poweredOn", T // 2),
        ("vm_prdm", "poweredOn", 2 * T),
        ("vm_vrdm", "poweredOn", 4 * T / 5),
        ("vm_cap", "poweredOn", 2 * T),
        ("vm_only_p", "poweredOn", T),
    ]:
        part.append(list(r))

    disk = wb.create_sheet("vDisk")
    disk.append(
        ["VM", "Powerstate", "Disk", "Capacity MiB", "Raw", "Raw Comp. Mode"]
    )
    for r in [
        ("vm_vmdk", "poweredOn", "Hard disk 1", T, False, ""),
        ("vm_prdm", "poweredOn", "Hard disk 1", T // 4, False, ""),
        ("vm_prdm", "poweredOn", "Hard disk 2", 2 * T, True, "physicalMode"),
        ("vm_vrdm", "poweredOn", "Hard disk 1", T, True, "virtualMode"),
        ("vm_cap", "poweredOn", "Hard disk 1", T // 2, False, ""),
        ("vm_only_p", "poweredOn", "Hard disk 1", T, True, "physicalMode"),
    ]:
        disk.append(list(r))
    wb.save(path)


here = Path(__file__).parent
build_baseline(here / "regression_baseline.xlsx")
build_baseline(here / "regression_no_part_powerstate.xlsx", part_powerstate=False)
build_disks(here / "regression_disks.xlsx")
print("wrote baseline, no_part_powerstate, disks")
