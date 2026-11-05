"""Regenerate the synthetic regression workbooks used by tests/regression.rs.

    pip install openpyxl
    python3 testdata/generate_testdata.py

Capacities are in MiB (1 TiB = 1048576 MiB). Expected (powered-on only):
  vm1  DC-A/CL1  vInfo 1 TiB,   vPartition 0.25+0.25 TiB -> 0.5 TiB
  vm2  DC-A/CL1  vInfo 0.5 TiB, vPartition 1 TiB         -> 0.5 TiB
  vm3  DC-A/CL2  vInfo 2 TiB,   no vPartition rows       -> 2.0 TiB
  vm4  DC-B/CL3  vInfo 1 TiB,   vPartition 0.75 TiB      -> 0.75 TiB
  vm5  DC-B/CL3  poweredOff, vInfo 1 TiB, vPartition 0.5 TiB (only with -i)
Total 3.75 TB / 4 VMs (4.25 TB / 5 VMs with -i; 4.5 TB with -d).
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


def build(path, part_powerstate=True):
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


here = Path(__file__).parent
build(here / "regression_baseline.xlsx")
build(here / "regression_no_part_powerstate.xlsx", part_powerstate=False)
