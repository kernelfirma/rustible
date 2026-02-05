# HPC Bare-Metal, Fabric, and Storage Requirements Matrix

Phase 2B of the HPC Initiative - Infrastructure layer requirements for bare-metal provisioning, high-performance fabrics, and parallel storage.

## Table of Contents

1. [Bare-Metal Provisioning](#1-bare-metal-provisioning)
2. [Out-of-Band Management (IPMI/Redfish)](#2-out-of-band-management-ipmiredfish)
3. [High-Performance Fabrics](#3-high-performance-fabrics-infinibandrdma)
4. [Parallel Filesystems](#4-parallel-filesystems)
5. [Cross-Cutting Requirements](#5-cross-cutting-requirements)
6. [Implementation Priorities](#6-implementation-priorities)

---

## 1. Bare-Metal Provisioning

### 1.1 Provisioning Systems Overview

| System | Status | Use Case | Key Features |
|--------|--------|----------|--------------|
| **Warewulf** | Active | HPC clusters | Stateless/stateful, VNFS images, overlays |
| **xCAT** | Maintenance mode (2024) | Large-scale HPC | Discovery, install, management |
| **Grendel** | Active | Modern HPC | Go-based, RESTful API, all-in-one |
| **Cobbler** | Active | General purpose | Kickstart/preseed management |
| **MAAS** | Active | Cloud-like bare metal | Ubuntu-focused, API-driven |

### 1.2 PXE Boot Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **DHCP Configuration** | Serve boot parameters to nodes | Correct next-server and filename options |
| **TFTP Server** | Serve bootloader binaries | pxelinux.0, grubx64.efi accessible |
| **Boot Menu Generation** | Per-node or group boot configurations | Dynamic menu based on MAC/hostname |
| **HTTP Boot Support** | UEFI HTTP boot (iPXE) | Chain loading from TFTP to HTTP |
| **DNS Integration** | Forward/reverse records for nodes | All nodes resolvable by hostname |

### 1.3 OS Imaging Requirements

| Mode | Description | Requirements |
|------|-------------|--------------|
| **Stateful** | Install to local disk | Kickstart/preseed templates, disk partitioning |
| **Stateless** | Boot from network image | VNFS/squashfs images, NFS/HTTP serving |
| **Hybrid** | Network boot + local scratch | Overlay configuration, persistent storage mapping |

### 1.4 Image Lifecycle Operations

| Operation | Warewulf | xCAT | Grendel |
|-----------|----------|------|---------|
| **Build Image** | `wwvnfs --chroot` | `genimage` | `grendel image build` |
| **Import Image** | `wwvnfs --import` | `copycds` | `grendel image import` |
| **Modify Image** | `chroot` into VNFS | `updatenode` | Container-based |
| **Deploy Image** | `wwsh provision set` | `nodeset` | `grendel provision` |
| **Rollback** | Keep previous VNFS | Image versioning | Git-versioned images |

### 1.5 Node Discovery Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **MAC Discovery** | Auto-detect new nodes | DHCP lease capture, BMC discovery |
| **Hardware Inventory** | Collect hardware details | CPU, memory, NICs, storage detected |
| **Auto-naming** | Assign hostnames by pattern | Rack/position-based naming |
| **Group Assignment** | Classify into node groups | GPU nodes, compute, login identified |

### 1.6 Configuration Files

**Warewulf:**
```
/etc/warewulf/
├── warewulf.conf         # Main configuration
├── nodes.conf            # Node definitions (deprecated, use wwctl)
├── defaults/
│   └── node.conf         # Default node attributes
└── ipxe/
    └── default.ipxe      # Boot template
```

**xCAT:**
```
/etc/xcat/
├── site.tab              # Site-wide settings
├── networks.tab          # Network definitions
├── passwd.tab            # Credentials
└── install/
    └── postscripts/      # Post-install scripts
```

**Grendel:**
```
/etc/grendel/
├── grendel.toml          # Main configuration
├── hosts.json            # Node definitions
└── images/               # Boot images
```

---

## 2. Out-of-Band Management (IPMI/Redfish)

### 2.1 Protocol Comparison

| Feature | IPMI 2.0 | Redfish |
|---------|----------|---------|
| **Protocol** | UDP/623, custom | HTTPS/RESTful |
| **Data Format** | Binary | JSON |
| **Security** | Basic auth, weak encryption | TLS, OAuth, RBAC |
| **Scalability** | Limited | High (async operations) |
| **Discovery** | None | SSDP |
| **Standardization** | Fixed commands | Extensible schemas |

### 2.2 Power Control Requirements

| Operation | IPMI Command | Redfish Endpoint | Acceptance Criteria |
|-----------|--------------|------------------|---------------------|
| **Power On** | `chassis power on` | `POST /Actions/ComputerSystem.Reset` | Node boots within timeout |
| **Power Off** | `chassis power off` | `POST /Actions/ComputerSystem.Reset` | Graceful or forced shutdown |
| **Power Cycle** | `chassis power cycle` | Reset action with type | Complete cycle confirmed |
| **Power Status** | `chassis power status` | `GET /Systems/{id}` | Current state returned |
| **PXE Boot Once** | `chassis bootdev pxe` | `PATCH /Systems/{id}` | Next boot from network |

### 2.3 Sensor & Health Monitoring

| Sensor Type | IPMI | Redfish | Use Case |
|-------------|------|---------|----------|
| **Temperature** | `sdr type Temperature` | `/Thermal` | Cooling validation |
| **Fan Speed** | `sdr type Fan` | `/Thermal/Fans` | Airflow monitoring |
| **Power Draw** | `dcmi power reading` | `/Power` | Energy accounting |
| **Memory ECC** | SEL events | `/Memory` | Error detection |
| **Drive Health** | Via enclosure | `/Storage/Drives` | Predictive maintenance |

### 2.4 BMC Configuration Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Network Configuration** | BMC IP/VLAN assignment | Static or DHCP, dedicated VLAN |
| **User Management** | Admin credentials | Role-based access, password rotation |
| **Alert Configuration** | SNMP traps, email | Events forwarded to monitoring |
| **Firmware Updates** | BMC firmware lifecycle | Staged updates with rollback |
| **Serial Console** | SOL (Serial over LAN) | Remote console access |

### 2.5 Vendor-Specific Considerations

| Vendor | BMC Name | IPMI Extensions | Redfish Support |
|--------|----------|-----------------|-----------------|
| **Dell** | iDRAC | RACADM CLI | Full OEM extensions |
| **HPE** | iLO | HPONCFG | iLO RESTful API |
| **Lenovo** | XCC/IMM | ASU | Full Redfish |
| **Supermicro** | SMC IPMI | SMCIPMItool | Redfish OOB |
| **AMD** | Various | Standard | Growing support |

### 2.6 Automation Requirements

| Requirement | Description | Implementation |
|-------------|-------------|----------------|
| **Bulk Operations** | Power control many nodes | Parallel IPMI/Redfish calls |
| **Inventory Collection** | Hardware discovery | FRU data, Redfish inventory |
| **Event Processing** | SEL/event log handling | Log aggregation, alerting |
| **Firmware Orchestration** | Coordinated updates | Staged rollout with validation |
| **Credential Management** | Secure password handling | Vault integration, rotation |

---

## 3. High-Performance Fabrics (InfiniBand/RDMA)

### 3.1 InfiniBand Components

| Component | Description | Management Requirements |
|-----------|-------------|------------------------|
| **HCA (Host Channel Adapter)** | Network card on compute nodes | Driver installation, firmware |
| **Switch** | Fabric interconnect | Subnet manager, firmware |
| **Subnet Manager (SM)** | Fabric controller | OpenSM/vendor SM configuration |
| **Gateway** | IB-to-Ethernet bridge | Routing configuration |

### 3.2 OpenSM Subnet Manager Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SM Placement** | Primary and standby SM | Failover within 30 seconds |
| **Partition Configuration** | Logical subnets (like VLANs) | Isolation between partitions |
| **LID Assignment** | Local identifier allocation | Consistent across reboots |
| **Routing Algorithm** | Path computation | Optimal for topology (fat-tree, etc.) |
| **QoS Configuration** | Service levels | Priority traffic classes |

### 3.3 OpenSM Configuration

**Primary configuration file: `/etc/opensm/opensm.conf`**

| Parameter | Description | Typical Value |
|-----------|-------------|---------------|
| `subnet_prefix` | Fabric identifier | `0xfe80000000000000` |
| `sm_priority` | SM election priority | 15 (master), 1 (standby) |
| `routing_engine` | Path algorithm | `ftree`, `minhop`, `updn` |
| `log_file` | SM logs | `/var/log/opensm.log` |
| `partition_config_file` | Partition definitions | `/etc/opensm/partitions.conf` |

**Partition configuration: `/etc/opensm/partitions.conf`**
```
Default=0x7fff,ipoib,sl=0: ALL
Compute=0x0001,sl=1: node[001-100]
Storage=0x0002,sl=2: oss[01-10],mds[01-02]
```

### 3.4 Fabric Operations

| Operation | Command/Method | Acceptance Criteria |
|-----------|----------------|---------------------|
| **Fabric Discovery** | `ibnetdiscover` | All nodes visible |
| **Link Health** | `iblinkinfo` | No errors, expected speed |
| **SM Status** | `sminfo` | Master SM identified |
| **Performance** | `perfquery` | Counters within tolerance |
| **Diagnostics** | `ibdiagnet` | No fabric errors |

### 3.5 RDMA Configuration

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Kernel Modules** | IB core, driver modules | `ib_core`, `mlx5_ib` loaded |
| **IP over IB (IPoIB)** | IP networking over IB | `ib0` interface configured |
| **Memory Locking** | Pinned memory for RDMA | `memlock` ulimit increased |
| **RDMA CM** | Connection management | `rdma_cm` module loaded |
| **ucx/libfabric** | User-space RDMA libraries | Installed and configured |

### 3.6 Fabric Speed Tiers

| Generation | Speed per Lane | 4x Link | Typical Use |
|------------|---------------|---------|-------------|
| **SDR** | 2.5 Gb/s | 10 Gb/s | Legacy |
| **DDR** | 5 Gb/s | 20 Gb/s | Legacy |
| **QDR** | 10 Gb/s | 40 Gb/s | Production |
| **FDR** | 14 Gb/s | 56 Gb/s | Production |
| **EDR** | 25 Gb/s | 100 Gb/s | Current |
| **HDR** | 50 Gb/s | 200 Gb/s | High-end |
| **NDR** | 100 Gb/s | 400 Gb/s | Cutting-edge |

---

## 4. Parallel Filesystems

### 4.1 Filesystem Comparison

| Feature | Lustre | BeeGFS | GPFS/Spectrum Scale |
|---------|--------|--------|---------------------|
| **License** | GPLv2 | Dual (GPLv2/Commercial) | Proprietary |
| **Max Capacity** | Exabytes | Petabytes | Exabytes |
| **Metadata** | Separate MDS | Distributed | Distributed |
| **POSIX Compliance** | High | High | Full |
| **Small Files** | Moderate | Good | Excellent |
| **Top500 Usage** | >60% | ~5% | ~30% |

### 4.2 Lustre Architecture Requirements

| Component | Description | Requirements |
|-----------|-------------|--------------|
| **MGS (Management Server)** | Configuration store | HA pair, small storage |
| **MDS (Metadata Server)** | Directory operations | High IOPS, HA recommended |
| **OSS (Object Storage Server)** | Data serving | High bandwidth, multiple OSTs |
| **Client** | Filesystem mount | Kernel module, network access |

### 4.3 Lustre Configuration Files

| File | Purpose | Location |
|------|---------|----------|
| **modprobe.d/lustre.conf** | Module parameters | `/etc/modprobe.d/` |
| **lnet.conf** | LNet configuration | `/etc/lnet.conf` |
| **fstab** | Mount points | `/etc/fstab` |
| **changelogs** | MDT changelog config | MGS configuration |

### 4.4 Lustre Operations

| Operation | Command | Use Case |
|-----------|---------|----------|
| **Format MGT/MDT/OST** | `mkfs.lustre` | Initial setup |
| **Mount Components** | `mount -t lustre` | Bring online |
| **Add OST** | `mkfs.lustre --ost`, `lctl` | Expand capacity |
| **Remove OST** | `lctl set_param ost.OST0001.active=0` | Maintenance |
| **Check Filesystem** | `lctl df`, `lfs df` | Health monitoring |
| **Quota Management** | `lfs setquota` | User/group limits |
| **Striping Config** | `lfs setstripe` | Performance tuning |

### 4.5 BeeGFS Architecture Requirements

| Component | Description | Requirements |
|-----------|-------------|--------------|
| **Management Service** | Cluster coordination | Single instance or HA |
| **Metadata Service** | Directory operations | SSD storage, replication optional |
| **Storage Service** | Data targets | Scalable, mirroring optional |
| **Client** | FUSE or kernel mount | Network access |
| **Mon (Monitoring)** | Metrics collection | Optional but recommended |

### 4.6 BeeGFS Configuration Files

| File | Purpose | Service |
|------|---------|---------|
| **beegfs-mgmtd.conf** | Management config | mgmtd |
| **beegfs-meta.conf** | Metadata service | meta |
| **beegfs-storage.conf** | Storage service | storage |
| **beegfs-client.conf** | Client mount | client |
| **beegfs-mounts.conf** | Auto-mount config | client |

### 4.7 GPFS/Spectrum Scale Requirements

| Component | Description | Requirements |
|-----------|-------------|--------------|
| **Cluster Manager** | Quorum and configuration | Odd number for quorum |
| **NSD (Network Shared Disk)** | Block device abstraction | Storage servers |
| **GUI/Monitoring** | Management interface | Optional management node |
| **CES (Cluster Export Services)** | NFS/SMB gateways | Protocol access nodes |

### 4.8 GPFS Configuration

| File/Command | Purpose |
|--------------|---------|
| **mmsdrfs** | Cluster descriptor |
| **mmcrcluster** | Create cluster |
| **mmcrnsd** | Create NSD |
| **mmcrfs** | Create filesystem |
| **mmlsconfig** | Show configuration |
| **mmchconfig** | Change parameters |

### 4.9 Storage Operations Matrix

| Operation | Lustre | BeeGFS | GPFS |
|-----------|--------|--------|------|
| **Expand Storage** | Add OST | Add storage target | Add NSD |
| **Rebalance** | Space release | `beegfs-ctl --rebalance` | `mmrestripefs` |
| **Check Health** | `lctl`, `lfs` | `beegfs-ctl --listnodes` | `mmhealth` |
| **Quota** | `lfs quota` | `beegfs-ctl --getquota` | `mmlsquota` |
| **Snapshots** | ZFS backend | Manual | `mmcrsnapshot` |

---

## 5. Cross-Cutting Requirements

### 5.1 Bring-Up Workflow

```
┌─────────────────────────────────────────────────────────────────────┐
│                     HPC Node Bring-Up Sequence                      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. BMC Configuration                                               │
│     ├── Network (IP/VLAN)                                          │
│     ├── Credentials                                                │
│     └── Alerts/Monitoring                                          │
│                                                                     │
│  2. Hardware Validation                                            │
│     ├── Memory test                                                │
│     ├── CPU/firmware check                                         │
│     └── Storage validation                                         │
│                                                                     │
│  3. Network Boot                                                   │
│     ├── DHCP/PXE discovery                                         │
│     ├── OS image deployment                                        │
│     └── Post-install configuration                                 │
│                                                                     │
│  4. High-Speed Fabric                                              │
│     ├── HCA driver/firmware                                        │
│     ├── IPoIB configuration                                        │
│     └── Fabric connectivity                                        │
│                                                                     │
│  5. Parallel Filesystem                                            │
│     ├── Client packages                                            │
│     ├── Mount configuration                                        │
│     └── Access validation                                          │
│                                                                     │
│  6. Scheduler Registration                                         │
│     ├── Node configuration                                         │
│     ├── Resource definition                                        │
│     └── Enable for jobs                                            │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 5.2 Maintenance Operations

| Operation | BMC | Provisioning | Fabric | Storage |
|-----------|-----|--------------|--------|---------|
| **Firmware Update** | iDRAC/iLO update | BIOS via PXE | HCA firmware | No direct impact |
| **Node Drain** | Power state check | Image hold | SM notification | Client unmount |
| **Hardware Swap** | Re-BMC configure | Re-provision | LID reassign | Rebuild if OSS |
| **Rolling Update** | Staged power cycle | Image push | No action | Client reconnect |

### 5.3 Recovery Procedures

| Failure | Detection | Recovery Action |
|---------|-----------|-----------------|
| **Node failure** | BMC heartbeat | Power cycle, re-provision |
| **BMC unresponsive** | IPMI timeout | Physical console |
| **Fabric partition** | SM alerts | Investigate switches |
| **MDS failure** | Mount errors | Failover to standby |
| **OST failure** | I/O errors | Mark OST inactive |

### 5.4 Dependencies Matrix

| Component | Depends On | Required For |
|-----------|------------|--------------|
| **BMC Network** | Management VLAN | All OOB operations |
| **PXE/DHCP** | BMC configured | OS deployment |
| **InfiniBand** | OS running | Parallel FS, MPI |
| **Parallel FS** | IB fabric, servers | Application I/O |
| **Scheduler** | All above | Job execution |

---

## 6. Implementation Priorities

### 6.1 Phase 1: Foundation (Required)

| Priority | Component | Rustible Module(s) |
|----------|-----------|-------------------|
| **P0** | IPMI power control | `ipmi_power`, `ipmi_boot` |
| **P0** | Redfish power control | `redfish_power`, `redfish_info` |
| **P0** | PXE boot configuration | `pxe_config`, `dhcp_host` |
| **P1** | BMC user management | `bmc_user` |
| **P1** | Sensor monitoring | `ipmi_sensor`, `redfish_health` |

### 6.2 Phase 2: Provisioning (High Priority)

| Priority | Component | Rustible Module(s) |
|----------|-----------|-------------------|
| **P1** | Warewulf integration | `warewulf_node`, `warewulf_vnfs` |
| **P1** | Node discovery | `node_discover` |
| **P2** | Image management | `os_image`, `image_deploy` |
| **P2** | Post-install config | `cloud_init`, `firstboot` |

### 6.3 Phase 3: High-Speed Fabric (Medium Priority)

| Priority | Component | Rustible Module(s) |
|----------|-----------|-------------------|
| **P2** | IB driver setup | `infiniband_driver` |
| **P2** | IPoIB configuration | `ipoib` |
| **P2** | OpenSM configuration | `opensm_config` |
| **P3** | Fabric diagnostics | `ib_diag`, `ib_health` |

### 6.4 Phase 4: Parallel Filesystems (Medium Priority)

| Priority | Component | Rustible Module(s) |
|----------|-----------|-------------------|
| **P2** | Lustre client mount | `lustre_mount` |
| **P2** | BeeGFS client | `beegfs_mount` |
| **P3** | Lustre OST management | `lustre_ost` |
| **P3** | Storage quota | `lustre_quota`, `beegfs_quota` |

### 6.5 Gap Analysis vs Current Rustible

| Capability | Current Status | Gap |
|------------|---------------|-----|
| IPMI commands | Not implemented | Need `ipmi` module |
| Redfish API | Not implemented | Need `redfish` module |
| PXE/DHCP | Generic `template` only | Need HPC-specific templates |
| InfiniBand | Not implemented | Need `infiniband` module family |
| Lustre | Not implemented | Need `lustre` module family |
| BeeGFS | Not implemented | Need `beegfs` module family |
| GPFS | Not implemented | Need `gpfs` module family |
| Warewulf | Not implemented | Need `warewulf` integration |

---

## References

- [Warewulf Documentation](https://warewulf.org/docs/main/contents/provisioning.html)
- [xCAT Project](https://xcat.org/)
- [Grendel Provisioning](https://grendel.readthedocs.io/)
- [DMTF Redfish Specification](https://www.dmtf.org/standards/redfish)
- [Red Hat IPMI/Redfish Automation](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/9/html/automating_system_administration_by_using_rhel_system_roles/remote-management-with-ipmi-and-redfish-by-using-the-rhel-mgmt-collection_automating-system-administration-by-using-rhel-system-roles)
- [OpenSM Configuration (SUSE)](https://documentation.suse.com/smart/network/html/subnet-manager-configuring/index.html)
- [OpenSM Configuration (Red Hat)](https://access.redhat.com/documentation/en-us/red_hat_enterprise_linux/7/html/networking_guide/sec-configuring_the_subnet_manager)
- [Lustre Filesystem](https://www.lustre.org/)
- [BeeGFS Documentation](https://www.weka.io/learn/glossary/file-storage/beegfs-parallel-file-system/)
- [IBM Spectrum Scale](https://www.ibm.com/docs/en/spectrum-scale)
