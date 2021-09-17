import asyncio
from asyncio import StreamReader, StreamWriter
from enum import Enum
from os import getenv
from typing import Union, Dict, Any, Optional

import aioredis
import pytest
from python_on_whales import docker, Image, Container

DNS_SUFFIX = '.dlemel8.xyz'


class TestPorts(Enum):
    BACKEND_PORT = 8080
    UNTUNNELER_PORT = 8899
    TUNNELER_PORT = 8888


class TunnelerType(Enum):
    TCP = 'tcp'
    DNS = 'dns'


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
                           extra_env_vars: Optional[Dict[str, Any]] = None) -> Container:
    if isinstance(local_port, TestPorts):
        local_port_value = local_port.value
    elif isinstance(local_port, int):
        local_port_value = local_port
    else:
        raise ValueError(f'unsupported local port type %s', type(local_port))

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
                                                       'IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS': 300000,
                                                       'CLIENT_SUFFIX': DNS_SUFFIX})
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
                                                       'CLIENT_SUFFIX': DNS_SUFFIX})
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
