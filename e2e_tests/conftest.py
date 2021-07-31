import asyncio
from asyncio import StreamReader, StreamWriter
from enum import Enum
from os import getenv
from typing import Union, Dict, Any, Optional

import pytest
from python_on_whales import docker, Image, Container


class TestPorts(Enum):
    BACKEND_PORT = 8080
    UNTUNNELER_PORT = 8899
    TUNNELER_PORT = 8888


class TunnelType(Enum):
    TCP = 'tcp'
    DNS = 'dns'


async def echo_handler(reader: StreamReader, writer: StreamWriter) -> None:
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
async def echo_backend_server():
    server = await asyncio.start_server(echo_handler, '127.0.0.1', TestPorts.BACKEND_PORT.value)
    address = server.sockets[0].getsockname()
    print(f'serving backend on {address}')

    async with server:
        task = asyncio.create_task(server.serve_forever())
        yield
        task.cancel()
        await server.wait_closed()


@pytest.fixture
async def redis_backend_server():
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
                           tunnel_type: TunnelType,
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
        [tunnel_type.value],
        name=container_name,
        detach=True,
        envs={
            'LOCAL_PORT': local_port_value,
            'REMOTE_PORT': remote_port.value,
            'REMOTE_ADDRESS': '127.0.0.1',
            'LOG_LEVEL': 'debug',
            **extra_env_vars,
        },
        networks=['host'],
    )


@pytest.fixture(scope='session')
def server_image() -> Image:
    image = build_tunneler_image('server', 'test_server')
    yield image
    docker.image.remove(image.id, force=True)


@pytest.fixture
def dns_server_container(server_image: Image) -> Container:
    container = run_tunneler_container(server_image,
                                       'test_server',
                                       TunnelType.DNS,
                                       TestPorts.UNTUNNELER_PORT,
                                       TestPorts.BACKEND_PORT,
                                       extra_env_vars={'READ_TIMEOUT_IN_MILLISECONDS': 100,
                                                       'IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS': 300000})
    yield container
    print(f'server {container.logs()=}')
    container.kill()
    container.remove(force=True)


@pytest.fixture
def tcp_server_container(server_image: Image) -> Container:
    container = run_tunneler_container(server_image,
                                       'test_server',
                                       TunnelType.TCP,
                                       TestPorts.UNTUNNELER_PORT,
                                       TestPorts.BACKEND_PORT)
    yield container
    print(f'server {container.logs()=}')
    container.kill()
    container.remove(force=True)


@pytest.fixture(scope='session')
def client_image() -> Image:
    image = build_tunneler_image('client', 'test_client')
    yield image
    docker.image.remove(image.id, force=True)


@pytest.fixture
def dns_client_container(client_image: Image) -> Container:
    container = run_tunneler_container(client_image,
                                       'test_client',
                                       TunnelType.DNS,
                                       TestPorts.TUNNELER_PORT,
                                       TestPorts.UNTUNNELER_PORT,
                                       extra_env_vars={'READ_TIMEOUT_IN_MILLISECONDS': 100,
                                                       'IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS': 30000})
    yield container
    print(f'client {container.logs()=}')
    container.kill()
    container.remove(force=True)


@pytest.fixture
def tcp_client_container(client_image: Image) -> Container:
    container = run_tunneler_container(client_image,
                                       'test_client',
                                       TunnelType.TCP,
                                       TestPorts.TUNNELER_PORT,
                                       TestPorts.UNTUNNELER_PORT)
    yield container
    print(f'client {container.logs()=}')
    container.kill()
    container.remove(force=True)
