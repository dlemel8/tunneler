import asyncio
from asyncio import StreamReader, StreamWriter
from copy import copy
from dataclasses import dataclass
from enum import Enum
from os import getenv
from pathlib import Path
from subprocess import run
from tempfile import TemporaryDirectory, NamedTemporaryFile
from typing import Union, Dict, Any, Optional, List, Tuple

import aioredis
import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from python_on_whales import docker, Image, Container

DNS_CLIENT_SUFFIX = '.dlemel8.xyz'
TLS_SERVER_NAME = 'server.tunneler'


class TestPorts(Enum):
    BACKEND_PORT = 8089
    UNTUNNELER_PORT = 8899
    TUNNELER_PORT = 8889


class TunnelerType(Enum):
    TCP = 'tcp'
    DNS = 'dns'
    TLS = 'tls'


class TunneledType(Enum):
    TCP = 'tcp'
    UDP = 'udp'


async def tcp_echo_handler(reader: StreamReader, writer: StreamWriter) -> None:
    while True:
        data = await reader.read(1024)
        if not data:
            break

        message = data.decode()
        peer_name = writer.get_extra_info('peername')
        print(f'{peer_name!r}: received {message!r}')
        writer.write(data)
        await writer.drain()

    print('close the connection')
    writer.close()


@pytest.fixture
async def tcp_echo_server():
    server = await asyncio.start_server(tcp_echo_handler, '127.0.0.1', TestPorts.BACKEND_PORT.value)
    address = server.sockets[0].getsockname()
    print(f'serving backend on {address}')

    async with server:
        task = asyncio.create_task(server.serve_forever())
        yield
        task.cancel()
        await server.wait_closed()


@pytest.fixture
async def redis_server():
    container = docker.run(
        'redis:6.0.12-alpine',
        name='redis_server',
        detach=True,
        publish=[(TestPorts.BACKEND_PORT.value, 6379)]
    )
    yield container
    print(f'redis server {container.logs()=}')
    container.kill()
    container.remove(force=True)


class EchoUdpServerProtocol(asyncio.DatagramProtocol):
    def __init__(self):
        self._transport = None

    def connection_made(self, transport):
        self._transport = transport

    def datagram_received(self, data, addr):
        message = data.decode()
        print(f'received {message} from {addr}')
        self._transport.sendto(data, addr)
        print(f'sent {message} to {addr}')


@pytest.fixture
async def udp_echo_server():
    loop = asyncio.get_running_loop()
    transport, protocol = await loop.create_datagram_endpoint(lambda: EchoUdpServerProtocol(),
                                                              local_addr=('127.0.0.1', TestPorts.BACKEND_PORT.value))
    print(f'serving {protocol} backend on {TestPorts.BACKEND_PORT.value}')
    yield transport
    transport.close()


def build_tunneler_image(executable: str, image_name: str) -> Image:
    cache_from_registry = getenv('CACHE_FROM_REGISTRY')
    cache_from = f'type=registry,ref={cache_from_registry}' if cache_from_registry else None
    return docker.build(
        context_path='.',
        tags=image_name,
        cache_from=cache_from,
        cache_to='type=inline',
        load=True,
        build_args={'EXECUTABLE': executable}
    )


def run_tunneler_container(image: Image,
                           container_name: str,
                           tunneler_type: TunnelerType,
                           tunneled_type: TunneledType,
                           local_port: Union[TestPorts, int],
                           remote_port: TestPorts,
                           extra_env_vars: Optional[Dict[str, Any]] = None,
                           volumes: Optional[List[Tuple[str, str]]] = None) -> Container:
    if isinstance(local_port, TestPorts):
        local_port_value = local_port.value
    elif isinstance(local_port, int):
        local_port_value = local_port
    else:
        raise ValueError(f'unsupported local port type {type(local_port)}')

    extra_env_vars = extra_env_vars or {}

    return docker.run(
        image,
        [tunneler_type.value],
        name=container_name,
        detach=True,
        envs={
            'LOCAL_PORT': local_port_value,
            'REMOTE_PORT': remote_port.value,
            'REMOTE_ADDRESS': '127.0.0.1',
            'TUNNELED_TYPE': tunneled_type.value,
            'LOG_LEVEL': 'debug',
            **extra_env_vars,
        },
        networks=['host'],
        volumes=volumes or [],
    )


def print_log_and_delete_container(container: Container) -> None:
    print(f'{container.name} {container.logs()=}')
    container.kill()
    container.remove(force=True)


@pytest.fixture(scope='session')
def server_image() -> Image:
    image = build_tunneler_image('server', 'test_server')
    yield image
    docker.image.remove(image.id, force=True)


@pytest.fixture
def tcp_over_dns_server(server_image: Image) -> Container:
    container = run_tunneler_container(server_image,
                                       'test_server',
                                       TunnelerType.DNS,
                                       TunneledType.TCP,
                                       TestPorts.UNTUNNELER_PORT,
                                       TestPorts.BACKEND_PORT,
                                       extra_env_vars={'READ_TIMEOUT_IN_MILLISECONDS': 100,
                                                       'IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS': 300_000,
                                                       'CLIENT_SUFFIX': DNS_CLIENT_SUFFIX})
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def udp_over_dns_server(server_image: Image) -> Container:
    container = run_tunneler_container(server_image,
                                       'test_server',
                                       TunnelerType.DNS,
                                       TunneledType.UDP,
                                       TestPorts.UNTUNNELER_PORT,
                                       TestPorts.BACKEND_PORT,
                                       extra_env_vars={'READ_TIMEOUT_IN_MILLISECONDS': 100,
                                                       'IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS': 300_000,
                                                       'CLIENT_SUFFIX': DNS_CLIENT_SUFFIX})
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def tcp_over_tcp_server(server_image: Image) -> Container:
    container = run_tunneler_container(server_image,
                                       'test_server',
                                       TunnelerType.TCP,
                                       TunneledType.TCP,
                                       TestPorts.UNTUNNELER_PORT,
                                       TestPorts.BACKEND_PORT)
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def udp_over_tcp_server(server_image: Image) -> Container:
    container = run_tunneler_container(server_image,
                                       'test_server',
                                       TunnelerType.TCP,
                                       TunneledType.UDP,
                                       TestPorts.UNTUNNELER_PORT,
                                       TestPorts.BACKEND_PORT)
    yield container
    print_log_and_delete_container(container)


@pytest.fixture(scope='session')
def ca_private_ca() -> str:
    with NamedTemporaryFile() as key_file:
        private_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        pem = private_key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.TraditionalOpenSSL,
            encryption_algorithm=serialization.NoEncryption(),
        )
        key_file.write(pem)
        key_file.flush()
        yield key_file.name


@dataclass
class PkiDerPaths:
    ca_certificate: Path
    server_key: Path
    server_certificate: Path
    client_key: Path
    client_certificate: Path


@pytest.fixture(scope='session')
def pki(ca_private_ca: str) -> PkiDerPaths:
    with TemporaryDirectory() as pki_dir:
        run(f'bash pki.sh -k {ca_private_ca} -t {pki_dir} ca server client', shell=True, check=True)
        tmp_dir_path = Path(pki_dir)
        yield PkiDerPaths(
            tmp_dir_path.joinpath('ca.crt.der'),
            tmp_dir_path.joinpath('server.key.der'),
            tmp_dir_path.joinpath('server.crt.der'),
            tmp_dir_path.joinpath('client.key.der'),
            tmp_dir_path.joinpath('client.crt.der'),
        )


@pytest.fixture
def tcp_over_tls_server(server_image: Image, pki: PkiDerPaths) -> Container:
    container = run_tunneler_container(server_image,
                                       'test_server',
                                       TunnelerType.TLS,
                                       TunneledType.TCP,
                                       TestPorts.UNTUNNELER_PORT,
                                       TestPorts.BACKEND_PORT,
                                       extra_env_vars={'CA_CERT': '/ca_certificate',
                                                       'KEY': '/server_key',
                                                       'CERT': '/server_certificate',
                                                       'SERVER_HOSTNAME': TLS_SERVER_NAME},
                                       volumes=[(str(pki.ca_certificate), '/ca_certificate'),
                                                (str(pki.server_key), '/server_key'),
                                                (str(pki.server_certificate), '/server_certificate')])
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def udp_over_tls_server(server_image: Image, pki: PkiDerPaths) -> Container:
    container = run_tunneler_container(server_image,
                                       'test_server',
                                       TunnelerType.TLS,
                                       TunneledType.UDP,
                                       TestPorts.UNTUNNELER_PORT,
                                       TestPorts.BACKEND_PORT,
                                       extra_env_vars={'CA_CERT': '/ca_certificate',
                                                       'KEY': '/server_key',
                                                       'CERT': '/server_certificate',
                                                       'SERVER_HOSTNAME': TLS_SERVER_NAME},
                                       volumes=[(str(pki.ca_certificate), '/ca_certificate'),
                                                (str(pki.server_key), '/server_key'),
                                                (str(pki.server_certificate), '/server_certificate')])
    yield container
    print_log_and_delete_container(container)


@pytest.fixture(scope='session')
def client_image() -> Image:
    image = build_tunneler_image('client', 'test_client')
    yield image
    docker.image.remove(image.id, force=True)


@pytest.fixture
def tcp_over_dns_client(client_image: Image) -> Container:
    container = run_tunneler_container(client_image,
                                       'test_client',
                                       TunnelerType.DNS,
                                       TunneledType.TCP,
                                       TestPorts.TUNNELER_PORT,
                                       TestPorts.UNTUNNELER_PORT,
                                       extra_env_vars={'READ_TIMEOUT_IN_MILLISECONDS': 100,
                                                       'IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS': 30000,
                                                       'CLIENT_SUFFIX': DNS_CLIENT_SUFFIX})
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def udp_over_dns_client(client_image: Image) -> Container:
    container = run_tunneler_container(client_image,
                                       'test_client',
                                       TunnelerType.DNS,
                                       TunneledType.UDP,
                                       TestPorts.TUNNELER_PORT,
                                       TestPorts.UNTUNNELER_PORT,
                                       extra_env_vars={'READ_TIMEOUT_IN_MILLISECONDS': 100,
                                                       'IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS': 30000,
                                                       'CLIENT_SUFFIX': DNS_CLIENT_SUFFIX})
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def tcp_over_tcp_client(client_image: Image) -> Container:
    container = run_tunneler_container(client_image,
                                       'test_client',
                                       TunnelerType.TCP,
                                       TunneledType.TCP,
                                       TestPorts.TUNNELER_PORT,
                                       TestPorts.UNTUNNELER_PORT)
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def udp_over_tcp_client(client_image: Image) -> Container:
    container = run_tunneler_container(client_image,
                                       'test_client',
                                       TunnelerType.TCP,
                                       TunneledType.UDP,
                                       TestPorts.TUNNELER_PORT,
                                       TestPorts.UNTUNNELER_PORT)
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def tcp_over_tls_client(client_image: Image, pki: PkiDerPaths) -> Container:
    container = run_tunneler_container(client_image,
                                       'test_client',
                                       TunnelerType.TLS,
                                       TunneledType.TCP,
                                       TestPorts.TUNNELER_PORT,
                                       TestPorts.UNTUNNELER_PORT,
                                       extra_env_vars={'CA_CERT': '/ca_certificate',
                                                       'KEY': '/client_key',
                                                       'CERT': '/client_certificate',
                                                       'SERVER_HOSTNAME': TLS_SERVER_NAME},
                                       volumes=[(str(pki.ca_certificate), '/ca_certificate'),
                                                (str(pki.client_key), '/client_key'),
                                                (str(pki.client_certificate), '/client_certificate')])
    yield container
    print_log_and_delete_container(container)


@pytest.fixture
def udp_over_tls_client(client_image: Image, pki: PkiDerPaths) -> Container:
    container = run_tunneler_container(client_image,
                                       'test_client',
                                       TunnelerType.TLS,
                                       TunneledType.UDP,
                                       TestPorts.TUNNELER_PORT,
                                       TestPorts.UNTUNNELER_PORT,
                                       extra_env_vars={'CA_CERT': '/ca_certificate',
                                                       'KEY': '/client_key',
                                                       'CERT': '/client_certificate',
                                                       'SERVER_HOSTNAME': TLS_SERVER_NAME},
                                       volumes=[(str(pki.ca_certificate), '/ca_certificate'),
                                                (str(pki.client_key), '/client_key'),
                                                (str(pki.client_certificate), '/client_certificate')])
    yield container
    print_log_and_delete_container(container)


async def run_test_tcp_single_client_single_short_echo() -> None:
    message_to_send = 'bla'
    reader, writer = await asyncio.open_connection('127.0.0.1', TestPorts.TUNNELER_PORT.value)

    writer.write(message_to_send.encode())
    await writer.drain()
    data = await reader.readexactly(len(message_to_send))
    received_message = data.decode()

    writer.close()
    await writer.wait_closed()

    assert received_message == message_to_send


async def run_test_tcp_single_client_multiple_short_echo() -> None:
    messages_to_send = ['bla', 'bli', 'blu']
    reader, writer = await asyncio.open_connection('127.0.0.1', TestPorts.TUNNELER_PORT.value)

    received_messages = []
    for message in messages_to_send:
        writer.write(message.encode())
        await writer.drain()
        data = await reader.readexactly(len(message))
        received_messages.append(data.decode())

    writer.close()
    await writer.wait_closed()

    assert received_messages == messages_to_send


async def run_test_tcp_single_client_single_long_echo() -> None:
    message_to_send = 'bla' * 10_000
    reader, writer = await asyncio.open_connection('127.0.0.1', TestPorts.TUNNELER_PORT.value)

    writer.write(message_to_send.encode())
    await writer.drain()
    data = await reader.readexactly(len(message_to_send))
    received_message = data.decode()
    writer.close()
    await writer.wait_closed()

    assert received_message == message_to_send


async def run_test_tcp_multiple_clients_single_short_echo() -> None:
    message_to_send = 'bla'
    readers, writers = [], []
    for _ in range(3):
        reader, writer = await asyncio.open_connection('127.0.0.1', TestPorts.TUNNELER_PORT.value)
        readers.append(reader)
        writers.append(writer)

    write_tasks = []
    for writer in writers:
        writer.write(message_to_send.encode())
        write_tasks.append(writer.drain())
    await asyncio.gather(*write_tasks)

    read_tasks = []
    for reader in readers:
        read_tasks.append(reader.readexactly(len(message_to_send)))
    received_messages = [data.decode() for data in await asyncio.gather(*read_tasks)]

    wait_close_tasks = []
    for writer in writers:
        writer.close()
        wait_close_tasks.append(writer.wait_closed())
    await asyncio.gather(*wait_close_tasks)

    for message in received_messages:
        assert message == message_to_send


async def run_test_tcp_multiple_tunnels_single_short_echo(another_client_container: Container) -> None:
    try:
        message_to_send = 'bla'
        readers, writers = [], []
        for i in range(2):
            reader, writer = await asyncio.open_connection('127.0.0.1', TestPorts.TUNNELER_PORT.value + i)
            readers.append(reader)
            writers.append(writer)

        write_tasks = []
        for writer in writers:
            writer.write(message_to_send.encode())
            write_tasks.append(writer.drain())
        await asyncio.gather(*write_tasks)

        read_tasks = []
        for reader in readers:
            read_tasks.append(reader.readexactly(len(message_to_send)))
        received_messages = [data.decode() for data in await asyncio.gather(*read_tasks)]

        wait_close_tasks = []
        for writer in writers:
            writer.close()
            wait_close_tasks.append(writer.wait_closed())
        await asyncio.gather(*wait_close_tasks)

        for message in received_messages:
            assert message == message_to_send

    finally:
        print_log_and_delete_container(another_client_container)


async def run_test_tcp_server_long_response_and_empty_acks() -> None:
    redis = aioredis.from_url(f'redis://127.0.0.1:{TestPorts.TUNNELER_PORT.value}/0')
    result = await redis.info()
    assert result


class EchoClientProtocol(asyncio.DatagramProtocol):
    def __init__(self, messages_to_send: List[str], communication_done: asyncio.Future):
        self._messages_to_send = copy(messages_to_send)
        self._communication_done = communication_done
        self._transport = None
        self._received_messages = []

    def connection_made(self, transport):
        self._transport = transport
        self._send_message()

    def datagram_received(self, data, addr):
        received_message = data.decode()
        print(f'received {received_message} from {addr}')
        self._received_messages.append(received_message)

        if self._messages_to_send:
            self._send_message()
        else:
            self._communication_done.set_result(self._received_messages)

    def _send_message(self):
        message_to_send = self._messages_to_send.pop(0)
        self._transport.sendto(message_to_send.encode())
        print(f'sent {message_to_send}')


async def run_test_udp_single_client_single_short_echo() -> None:
    message_to_send = 'bla'

    loop = asyncio.get_running_loop()
    communication_done = loop.create_future()
    transport, _ = await loop.create_datagram_endpoint(
        lambda: EchoClientProtocol([message_to_send], communication_done),
        remote_addr=('127.0.0.1', TestPorts.TUNNELER_PORT.value))

    try:
        received_message = (await communication_done).pop()
        assert received_message == message_to_send
    finally:
        transport.close()


async def run_test_udp_single_client_multiple_short_echo() -> None:
    messages_to_send = ['bla', 'bli', 'blu']

    loop = asyncio.get_running_loop()
    communication_done = loop.create_future()
    transport, _ = await loop.create_datagram_endpoint(
        lambda: EchoClientProtocol(messages_to_send, communication_done),
        remote_addr=('127.0.0.1', TestPorts.TUNNELER_PORT.value))

    try:
        received_messages = await communication_done
        assert received_messages == messages_to_send
    finally:
        transport.close()


async def run_test_udp_single_client_single_long_echo(payload_ratio: int) -> None:
    message_to_send = 'bla' * payload_ratio

    loop = asyncio.get_running_loop()
    communication_done = loop.create_future()
    transport, _ = await loop.create_datagram_endpoint(
        lambda: EchoClientProtocol([message_to_send], communication_done),
        remote_addr=('127.0.0.1', TestPorts.TUNNELER_PORT.value))

    try:
        received_message = (await communication_done).pop()
        assert received_message == message_to_send
    finally:
        transport.close()


async def run_test_udp_multiple_tunnels_single_short_echo(another_client_container: Container) -> None:
    try:
        message_to_send = 'bla'
        loop = asyncio.get_running_loop()

        communication_dones, transports = [], []
        try:
            for i in range(2):
                communication_done = loop.create_future()
                transport, _ = await loop.create_datagram_endpoint(
                    lambda: EchoClientProtocol([message_to_send], communication_done),
                    remote_addr=('127.0.0.1', TestPorts.TUNNELER_PORT.value + i))
                communication_dones.append(communication_done)
                transports.append(transport)

            for communication_done in communication_dones:
                received_message = (await communication_done).pop()
                assert received_message == message_to_send
        finally:
            for transport in transports:
                transport.close()
    finally:
        print_log_and_delete_container(another_client_container)
