"""Regenerate the synthetic regression workbooks used by tests/regression.rs.

    pip install openpyxl
    python3 testdata/generate_testdata.py

Capacities are in MiB (1 TiB = 1048576 MiB).

Baseline (no vDisk): see prior docs — 3.75 TB / 4 VMs powered-on.

regression_disks.xlsx (phase 1):
  vm_vmdk 0.5, vm_prdm 0.25 (pRDM excluded), vm_vrdm 0.8, vm_cap 0.5, vm_only_p excluded
  → 2.05 TB / 4 VMs; --keep-rdm → 4.8 TB / 5 VMs

regression_disks2.xlsx (phase 2):
  vm_ind   — VMDK 0.5 + independentPersistent 1.0; guest 1.2; vInfo 1.5
             default eligible 0.5 → 0.5; --keep-independent → min(1.5,1.2)=1.2
  vm_share_a / vm_share_b — shared multi-writer UUID disk 1.0 TiB each side; guest 1.0
             counted once → total 1.0 across both VMs
  vm_tools — toolsNotRunning, guest 0.1, vInfo 2.0, VMDK 2.0 → ignore guest → 2.0
  vm_nopart — poweredOn, toolsOk, VMDK 0.5, no vPartition → 0.5 + exception
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
    for r in [
        ("vm_vmdk", "poweredOn", "DC-D", "CL1", T),
        ("vm_prdm", "poweredOn", "DC-D", "CL1", 9 * T / 4),
        ("vm_vrdm", "poweredOn", "DC-D", "CL2", T),
        ("vm_cap", "poweredOn", "DC-D", "CL2", 2 * T),
        ("vm_only_p", "poweredOn", "DC-D", "CL3", T),
    ]:
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
    disk.append(["VM", "Powerstate", "Disk", "Capacity MiB", "Raw", "Raw Comp. Mode", "Disk Mode", "Sharing", "Disk UUID"])
    for r in [
        ("vm_vmdk", "poweredOn", "Hard disk 1", T, False, "", "persistent", "", ""),
        ("vm_prdm", "poweredOn", "Hard disk 1", T // 4, False, "", "persistent", "", ""),
        ("vm_prdm", "poweredOn", "Hard disk 2", 2 * T, True, "physicalMode", "persistent", "", ""),
        ("vm_vrdm", "poweredOn", "Hard disk 1", T, True, "virtualMode", "persistent", "", ""),
        ("vm_cap", "poweredOn", "Hard disk 1", T // 2, False, "", "persistent", "", ""),
        ("vm_only_p", "poweredOn", "Hard disk 1", T, True, "physicalMode", "persistent", "", ""),
    ]:
        disk.append(list(r))
    wb.save(path)


def build_disks2(path):
    wb = Workbook()
    info = wb.active
    info.title = "vInfo"
    info.append(["VM", "Powerstate", "Datacenter", "Cluster", "In Use MiB", "Tools"])
    for r in [
        ("vm_ind", "poweredOn", "DC-E", "CL1", 1.5 * T, "toolsOk"),
        ("vm_share_a", "poweredOn", "DC-E", "CL2", T, "toolsOk"),
        ("vm_share_b", "poweredOn", "DC-E", "CL2", T, "toolsOk"),
        ("vm_tools", "poweredOn", "DC-E", "CL3", 2 * T, "toolsNotRunning"),
        ("vm_nopart", "poweredOn", "DC-E", "CL3", T // 2, "toolsOk"),
    ]:
        info.append(list(r))
    part = wb.create_sheet("vPartition")
    part.append(["VM", "Powerstate", "Consumed MiB"])
    for r in [
        ("vm_ind", "poweredOn", 1.2 * T),
        ("vm_share_a", "poweredOn", T),
        ("vm_share_b", "poweredOn", T),
        ("vm_tools", "poweredOn", 0.1 * T),
        # vm_nopart: intentionally no rows
    ]:
        part.append(list(r))
    disk = wb.create_sheet("vDisk")
    disk.append(["VM", "Powerstate", "Disk", "Capacity MiB", "Raw", "Raw Comp. Mode", "Disk Mode", "Sharing", "Disk UUID"])
    for r in [
        ("vm_ind", "poweredOn", "Hard disk 1", T // 2, False, "", "persistent", "", "U-IND-1"),
        ("vm_ind", "poweredOn", "Hard disk 2", T, False, "", "independentPersistent", "", "U-IND-2"),
        ("vm_share_a", "poweredOn", "Hard disk 1", T, False, "", "persistent", "sharingMultiWriter", "U-SHARE-1"),
        ("vm_share_b", "poweredOn", "Hard disk 1", T, False, "", "persistent", "sharingMultiWriter", "U-SHARE-1"),
        ("vm_tools", "poweredOn", "Hard disk 1", 2 * T, False, "", "persistent", "", "U-TOOLS"),
        ("vm_nopart", "poweredOn", "Hard disk 1", T // 2, False, "", "persistent", "", "U-NOPART"),
    ]:
        disk.append(list(r))
    wb.save(path)


def build_disk_summary(path, second_file=False):
    """Shared independent attachments across files; caps masked by vInfo;
    an all-independent VM; and a disk that is both pRDM and independent.
    Default total across both files: 1.375 TiB / 4 VMs.
    --keep-independent: 2.125 TiB / 5 VMs (+0.75, not 7.75 provisioned TiB).
    """
    wb = Workbook()
    info = wb.active
    info.title = "vInfo"
    info.append(["VM", "Powerstate", "Datacenter", "Cluster", "In Use MiB", "Tools"])
    part = wb.create_sheet("vPartition")
    part.append(["VM", "Powerstate", "Consumed MiB"])
    disk = wb.create_sheet("vDisk")
    disk.append(["VM", "Powerstate", "Disk", "Capacity MiB", "Raw Comp. Mode", "Disk Mode", "Sharing", "Disk UUID"])
    if second_file:
        vms = [("vm_b", 1, 0.75)]
        disks = [
            ("vm_b", "boot", 0.5, "", "persistent", "", "BOOT-B"),
            ("vm_b", "shared", 4, "", "independentPersistent", "sharingMultiWriter", " shared-independent "),
        ]
    else:
        vms = [("vm_a", 1, 1.25), ("vm_floor", 0.125, 2),
               ("vm_only_independent", 0.5, 0.25), ("vm_both", 0.5, 0.5)]
        disks = [
            ("vm_a", "boot", 0.5, "", "persistent", "", "BOOT-A"),
            ("vm_a", "shared", 4, "", "independentPersistent", "sharingMultiWriter", "SHARED-INDEPENDENT"),
            ("vm_a", "shared", 4, "", "independentPersistent", "sharingMultiWriter", "SHARED-INDEPENDENT"),
            ("vm_floor", "boot", 0.25, "", "persistent", "", "BOOT-FLOOR"),
            ("vm_floor", "independent", 1, "", "independentPersistent", "", "INDEPENDENT-FLOOR"),
            ("vm_only_independent", "independent", 2, "", "independentPersistent", "", "INDEPENDENT-ONLY"),
            ("vm_both", "boot", 0.25, "", "persistent", "", "BOOT-BOTH"),
            ("vm_both", "independent-pRDM", 0.75, "physicalMode", "independentPersistent", "", "BOTH"),
        ]
    for vm, in_use, consumed in vms:
        info.append([vm, "poweredOn", "DC-S", "CL", in_use * T, "toolsOk"])
        part.append([vm, "poweredOn", consumed * T])
    for vm, label, size, raw, mode, sharing, uuid in disks:
        disk.append([vm, "poweredOn", label, size * T, raw, mode, sharing, uuid])
    wb.save(path)


def build_uuid_join(path, second_file=False):
    """Duplicate VM names across vCenters/files, joined by UUID.
    File A: web01 (DC-A, inst i-1) guest 0.5, web01 (DC-B, inst i-2) guest 1.0,
            db-a with multi-writer disk SHARED-X (guest 1.0).
    File B: web01 (DC-C, inst i-3) guest 0.25 — child sheets carry only VM UUID,
            db-b sharing SHARED-X (deduped to 0).
    Expected A+B: 2.75 TiB / 5 VMs. Joining by name would give a different total.
    """
    wb = Workbook()
    info = wb.active
    info.title = "vInfo"
    info.append(["VM", "Powerstate", "Datacenter", "Cluster", "In Use MiB", "VM UUID", "VM Instance UUID"])
    part = wb.create_sheet("vPartition")
    disk = wb.create_sheet("vDisk")
    if second_file:
        part.append(["VM", "Powerstate", "Consumed MiB", "VM UUID"])
        disk.append(["VM", "Powerstate", "Disk", "Capacity MiB", "Sharing", "Disk UUID", "VM UUID"])
        vms = [("web01", "DC-C", T, "b-3", "i-3"), ("db-b", "DC-C", T, "b-db-b", "i-db-b")]
        parts = [("web01", T // 4, "b-3"), ("db-b", T, "b-db-b")]
        disks = [("web01", "boot", T, "", "W3", "b-3"), ("db-b", "shared", T, "sharingMultiWriter", "SHARED-X", "b-db-b")]
        for r in vms:
            info.append([r[0], "poweredOn", r[1], "CL", r[2], r[3], r[4]])
        for vm, cap, u in parts:
            part.append([vm, "poweredOn", cap, u])
        for vm, label, cap, sharing, duuid, u in disks:
            disk.append([vm, "poweredOn", label, cap, sharing, duuid, u])
    else:
        part.append(["VM", "Powerstate", "Consumed MiB", "VM UUID", "VM Instance UUID"])
        disk.append(["VM", "Powerstate", "Disk", "Capacity MiB", "Sharing", "Disk UUID", "VM UUID", "VM Instance UUID"])
        vms = [("web01", "DC-A", T, "b-1", "i-1"), ("web01", "DC-B", 2 * T, "b-2", "i-2"),
               ("db-a", "DC-A", T, "b-db-a", "i-db-a")]
        parts = [("web01", T // 2, "b-1", "i-1"), ("web01", T, "b-2", "i-2"), ("db-a", T, "b-db-a", "i-db-a")]
        disks = [("web01", "boot", T, "", "W1", "b-1", "i-1"), ("web01", "boot", 2 * T, "", "W2", "b-2", "i-2"),
                 ("db-a", "shared", T, "sharingMultiWriter", "SHARED-X", "b-db-a", "i-db-a")]
        for r in vms:
            info.append([r[0], "poweredOn", r[1], "CL", r[2], r[3], r[4]])
        for vm, cap, b, i in parts:
            part.append([vm, "poweredOn", cap, b, i])
        for vm, label, cap, sharing, duuid, b, i in disks:
            disk.append([vm, "poweredOn", label, cap, sharing, duuid, b, i])
    wb.save(path)


def build_no_uuid_duplicate(path):
    """Old export without UUID columns and a duplicated name: app01 x2 (1 TiB each),
    one vPartition row 0.5 attaches to the first record only; solo 0.25.
    Expected 1.75 TiB / 3 VMs (joining by name to both records gave 1.25)."""
    wb = Workbook()
    info = wb.active
    info.title = "vInfo"
    info.append(["VM", "Powerstate", "Datacenter", "Cluster", "In Use MiB"])
    info.append(["app01", "poweredOn", "DC-A", "CL", T])
    info.append(["app01", "poweredOn", "DC-B", "CL", T])
    info.append(["solo", "poweredOn", "DC-A", "CL", T])
    part = wb.create_sheet("vPartition")
    part.append(["VM", "Powerstate", "Consumed MiB"])
    part.append(["app01", "poweredOn", T // 2])
    part.append(["solo", "poweredOn", T // 4])
    wb.save(path)


def build_anon_filter(path):
    """Filters and site maps must match real names under --anonymize.
    DC-Keep/CL-Keep: keep01 0.5; DC-Keep/CL-Drop: drop01 1.0; DC-Drop: other01 2.0."""
    wb = Workbook()
    info = wb.active
    info.title = "vInfo"
    info.append(["VM", "Powerstate", "Datacenter", "Cluster", "In Use MiB", "VM UUID"])
    info.append(["keep01", "poweredOn", "DC-Keep", "CL-Keep", T // 2, "u-1"])
    info.append(["drop01", "poweredOn", "DC-Keep", "CL-Drop", T, "u-2"])
    info.append(["other01", "poweredOn", "DC-Drop", "CL-X", 2 * T, "u-3"])
    wb.save(path)


here = Path(__file__).parent
build_baseline(here / "regression_baseline.xlsx")
build_baseline(here / "regression_no_part_powerstate.xlsx", part_powerstate=False)
build_disks(here / "regression_disks.xlsx")
build_disks2(here / "regression_disks2.xlsx")
build_disk_summary(here / "regression_disk_summary_a.xlsx")
build_disk_summary(here / "regression_disk_summary_b.xlsx", second_file=True)
build_uuid_join(here / "regression_uuid_a.xlsx")
build_uuid_join(here / "regression_uuid_b.xlsx", second_file=True)
build_no_uuid_duplicate(here / "regression_no_uuid_dup.xlsx")
build_anon_filter(here / "regression_anon_filter.xlsx")
print("wrote uuid_a, uuid_b, no_uuid_dup, anon_filter, baseline, no_part_powerstate, disks, disks2, disk_summary_a, disk_summary_b")
