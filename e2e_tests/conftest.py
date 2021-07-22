import asyncio
from asyncio import StreamReader, StreamWriter
from os import getenv

import pytest
import python_on_whales
from docker import DockerClient, from_env
from docker.models.containers import Container
from docker.models.images import Image

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
    server = await asyncio.start_server(echo_handler, '127.0.0.1', BACKEND_PORT)
    address = server.sockets[0].getsockname()
    print(f'serving backend on {address}')

    async with server:
        task = asyncio.create_task(server.serve_forever())
        yield
        task.cancel()
        await server.wait_closed()


@pytest.fixture(scope='session')
def docker_client() -> DockerClient:
    yield from_env()


@pytest.fixture(scope='session')
def server_image(docker_client: DockerClient) -> Image:
    cache_from_registry = getenv('CACHE_FROM_REGISTRY')
    cache_from = f'type=registry,ref={cache_from_registry}' if cache_from_registry else None
    image = python_on_whales.docker.build(context_path='.',
                                          tags='test_server',
                                          cache_from=cache_from,
                                          cache_to='type=inline',
                                          build_args={'EXECUTABLE': 'server'})

    yield image
    docker_client.images.remove(image.id, force=True)


@pytest.fixture
def server_container(docker_client: DockerClient, server_image: Image) -> Container:
    container = docker_client.containers.run(
        server_image.id,
        name='test_server',
        detach=True,
        environment={
            'TUNNEL_TYPE': 'Dns',
            'LOCAL_PORT': UNTUNNELER_PORT,
            'REMOTE_PORT': BACKEND_PORT,
            'REMOTE_ADDRESS': '127.0.0.1',
            'LOG_LEVEL': 'debug',
        },
        network_mode='host'
    )
    yield container
    print(f'server {container.logs()=}')
    container.stop(timeout=0)
    container.remove(force=True)


@pytest.fixture(scope='session')
def client_image(docker_client: DockerClient) -> Image:
    cache_from_registry = getenv('CACHE_FROM_REGISTRY')
    cache_from = f'type=registry,ref={cache_from_registry}' if cache_from_registry else None
    image = python_on_whales.docker.build(context_path='.',
                                          tags='test_client',
                                          cache_from=cache_from,
                                          cache_to='type=inline',
                                          build_args={'EXECUTABLE': 'client'})
    yield image
    docker_client.images.remove(image.id, force=True)


@pytest.fixture
def client_container(docker_client: DockerClient, client_image: Image) -> Container:
    container = docker_client.containers.run(
        client_image.id,
        name='test_client',
        detach=True,
        environment={
            'TUNNEL_TYPE': 'Dns',
            'LOCAL_PORT': TUNNELER_PORT,
            'REMOTE_PORT': UNTUNNELER_PORT,
            'REMOTE_ADDRESS': '127.0.0.1',
            'LOG_LEVEL': 'debug',
        },
        network_mode='host'
    )
    yield container
    print(f'client {container.logs()=}')
    container.stop(timeout=0)
    container.remove(force=True)
