use std::{
    collections::{BTreeMap, BinaryHeap},
    net::Ipv4Addr,
};

use anyhow::Error;
use jail::RunningJail;
use netzwerk::{
    interface::Interface,
    range::{broadcast, mask, range as ip_range},
    route,
    pf::Pf,
    nat::Nat,
};
use storage::{Storage, StorageEngine};

const NETWORK_STATE_STORAGE_KEY: &[u8] = b"NETWORK_STATE";
const CONTAINER_ADDRESS_STORAGE_KEY: &[u8] = b"CONTAINER_ADDRESS";
const DEFAULT_NETWORK: &str = "172.24.0.0/16";
const DEFAULT_BRIDGE: &str = "knast0";

type ContainerAddressStorage = BTreeMap<String, (String, Ipv4Addr, Ipv4Addr)>;

#[fehler::throws]
pub fn setup(
    storage: &Storage<impl StorageEngine>,
    key: impl AsRef<str>,
    jail: RunningJail,
    nat_interface: Option<impl AsRef<str>>,
) {
    let bridge = setup_bridge(storage)?;
    let host = setup_pair(storage, key, jail)?;
    let host_name = host.get_name()?;

    bridge.bridge_addm(&[host_name])?;

    if let Some(nat_interface) = nat_interface {
        let nat = Pf::new(nat_interface.as_ref())?;
        nat.add(DEFAULT_NETWORK)?;
    }
}

#[fehler::throws]
pub fn teardown(
    storage: &Storage<impl StorageEngine>,
    key: impl AsRef<str>,
) {
    let cache: ContainerAddressStorage = storage
        .get(NETWORK_STATE_STORAGE_KEY, CONTAINER_ADDRESS_STORAGE_KEY)?
        .ok_or_else(|| anyhow::anyhow!("Failed to read network state data"))?;
    let key: String = key.as_ref().into();
    let (iface, host, container) = cache
        .get(&key)
        .ok_or_else(|| anyhow::anyhow!("Failed to read network state data"))?;
    Interface::new(iface)?.destroy()?;
    release_addresses(storage, key)?;
    free_address(&storage, *host)?;
    free_address(&storage, *container)?;
}

#[fehler::throws]
fn setup_pair(
    storage: &Storage<impl StorageEngine>,
    key: impl AsRef<str>,
    jail: RunningJail,
) -> Interface {
    let host_address = get_address(&storage)?;
    let container_address = get_address(&storage)?;
    let broadcast = broadcast(DEFAULT_NETWORK)?.to_string();
    let mask = mask(DEFAULT_NETWORK)?.to_string();
    let pair_a = Interface::new("epair")?.create()?.address(
        &host_address.to_string(),
        &broadcast,
        &mask,
    )?;
    let name = pair_a.get_name()?;
    let len = name.len();
    let name_b = &[&name[..len - 1], "b"].join("");
    reserve_addresses(storage, key, name, (host_address, container_address))?;

    let pair_b = Interface::new(name_b)?;
    pair_b.vnet(jail.jid)?;

    super::utils::run_in_fork(|| {
        jail.attach()?;
        let pair_b = Interface::new(name_b)?;
        pair_b.address(&container_address.to_string(), &broadcast, &mask)?;
        route::add_default(&host_address.to_string())
    })?;

    pair_a
}

#[fehler::throws]
fn setup_bridge(storage: &Storage<impl StorageEngine>) -> Interface {
    let mut bridge = Interface::new(DEFAULT_BRIDGE)?;

    if !bridge.exists()? {
        let bridge_address = get_address(storage)?.to_string();
        let broadcast = broadcast(DEFAULT_NETWORK)?.to_string();
        let mask = mask(DEFAULT_NETWORK)?.to_string();

        bridge = Interface::new("bridge")?
            .create()?
            .name(DEFAULT_BRIDGE)?
            .address(&bridge_address, &broadcast, &mask)?;
    }

    bridge
}

#[fehler::throws]
fn get_address(storage: &Storage<impl StorageEngine>) -> Ipv4Addr {
    let maybe_heap: Option<BinaryHeap<Ipv4Addr>> =
        storage.get(NETWORK_STATE_STORAGE_KEY, DEFAULT_NETWORK.as_bytes())?;

    if let Some(heap) = maybe_heap {
        let mut new_heap = heap.clone();

        let mut address = new_heap
            .pop()
            .ok_or_else(|| anyhow::anyhow!("No addresses left"))?;
        if address.is_broadcast() {
            address = new_heap
                .pop()
                .ok_or_else(|| anyhow::anyhow!("No addresses left"))?;
        }

        if let Err(_) = storage.compare_and_swap(
            NETWORK_STATE_STORAGE_KEY,
            DEFAULT_NETWORK.as_bytes(),
            Some(heap),
            Some(new_heap),
        ) {
            return get_address(&storage)?;
        };

        address
    } else {
        let range = ip_range(DEFAULT_NETWORK)?;

        storage.compare_and_swap(
            NETWORK_STATE_STORAGE_KEY,
            DEFAULT_NETWORK.as_bytes(),
            None,
            Some(range),
        )?;
        get_address(&storage)?
    }
}

#[fehler::throws]
fn free_address(storage: &Storage<impl StorageEngine>, address: Ipv4Addr) {
    let maybe_heap: Option<BinaryHeap<Ipv4Addr>> =
        storage.get(NETWORK_STATE_STORAGE_KEY, DEFAULT_NETWORK.as_bytes())?;

    if let Some(heap) = maybe_heap {
        let mut new_heap = heap.clone();

        new_heap.push(address);

        if let Err(_) = storage.compare_and_swap(
            NETWORK_STATE_STORAGE_KEY,
            DEFAULT_NETWORK.as_bytes(),
            Some(heap),
            Some(new_heap),
        ) {
            free_address(&storage, address)?;
        };
    } else {
        let range = ip_range(DEFAULT_NETWORK)?;

        storage.compare_and_swap(
            NETWORK_STATE_STORAGE_KEY,
            DEFAULT_NETWORK.as_bytes(),
            None,
            Some(range),
        )?;
        free_address(&storage, address)?;
    }
}

#[fehler::throws]
fn reserve_addresses(
    storage: &Storage<impl StorageEngine>,
    key: impl AsRef<str>,
    interface: impl AsRef<str>,
    addresses: (Ipv4Addr, Ipv4Addr),
) {
    let maybe_cache: Option<ContainerAddressStorage> = storage
        .get(NETWORK_STATE_STORAGE_KEY, CONTAINER_ADDRESS_STORAGE_KEY)?;

    if let Some(cache) = maybe_cache {
        let mut new_cache = cache.clone();
        new_cache.insert(
            key.as_ref().into(),
            (interface.as_ref().into(), addresses.0, addresses.1),
        );

        if let Err(_) = storage.compare_and_swap(
            NETWORK_STATE_STORAGE_KEY,
            CONTAINER_ADDRESS_STORAGE_KEY,
            Some(cache),
            Some(new_cache),
        ) {
            reserve_addresses(storage, key, interface, addresses)?;
        };
    } else {
        let empty_cache: ContainerAddressStorage = BTreeMap::new();
        storage.compare_and_swap(
            NETWORK_STATE_STORAGE_KEY,
            CONTAINER_ADDRESS_STORAGE_KEY,
            None,
            Some(empty_cache),
        )?;
        reserve_addresses(storage, key, interface, addresses)?;
    }
}

#[fehler::throws]
fn release_addresses(
    storage: &Storage<impl StorageEngine>,
    key: impl AsRef<str>
) {
    let maybe_cache: Option<ContainerAddressStorage> = storage
        .get(NETWORK_STATE_STORAGE_KEY, CONTAINER_ADDRESS_STORAGE_KEY)?;

    if let Some(cache) = maybe_cache {
        let mut new_cache = cache.clone();
        let key: String = key.as_ref().into();
        new_cache.remove(&key);

        if let Err(_) = storage.compare_and_swap(
            NETWORK_STATE_STORAGE_KEY,
            CONTAINER_ADDRESS_STORAGE_KEY,
            Some(cache),
            Some(new_cache),
        ) {
            release_addresses(storage, key)?;
        };
    } else {
        let empty_cache: ContainerAddressStorage = BTreeMap::new();
        storage.compare_and_swap(
            NETWORK_STATE_STORAGE_KEY,
            CONTAINER_ADDRESS_STORAGE_KEY,
            None,
            Some(empty_cache),
        )?;
        release_addresses(storage, key)?;
    }
}
