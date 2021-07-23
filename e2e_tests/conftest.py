import asyncio
from asyncio import StreamReader, StreamWriter
from enum import Enum
from os import getenv

import pytest
from python_on_whales import docker, Image, Container


class TestPorts(Enum):
    BACKEND_PORT = 8080
    UNTUNNELER_PORT = 8899
    TUNNELER_PORT = 8888


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


def run_tunneler_container(image: Image, container_name: str, local_port: TestPorts, remote_port: TestPorts) -> Container:
    return docker.run(
        image,
        name=container_name,
        detach=True,
        envs={
            'TUNNEL_TYPE': 'Dns',
            'LOCAL_PORT': local_port.value,
            'REMOTE_PORT': remote_port.value,
            'REMOTE_ADDRESS': '127.0.0.1',
            'LOG_LEVEL': 'debug',
        },
        networks=['host'],
    )


@pytest.fixture(scope='session')
def server_image() -> Image:
    image = build_tunneler_image('server', 'test_server')
    yield image
    docker.image.remove(image.id, force=True)


@pytest.fixture
def server_container(server_image: Image) -> Container:
    container = run_tunneler_container(server_image, 'test_server', TestPorts.UNTUNNELER_PORT, TestPorts.BACKEND_PORT)
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
def client_container(client_image: Image) -> Container:
    container = run_tunneler_container(client_image, 'test_client', TestPorts.TUNNELER_PORT, TestPorts.UNTUNNELER_PORT)
    yield container
    print(f'client {container.logs()=}')
    container.kill()
    container.remove(force=True)
