import asyncio

import pytest

from e2e_tests.conftest import TUNNELER_PORT


@pytest.mark.asyncio
async def test_dns_tunnel_echo(echo_backend_server, server_container, client_container) -> None:
    message = 'bla'
    reader, writer = await asyncio.open_connection('127.0.0.1', TUNNELER_PORT)

    writer.write(message.encode())
    await writer.drain()
    data = await reader.read(1024)
    writer.close()
    await writer.wait_closed()

    assert data.decode() == message
