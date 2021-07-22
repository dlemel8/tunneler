import asyncio

import pytest

from e2e_tests.conftest import TUNNELER_PORT


@pytest.mark.asyncio
async def test_dns_tunnel_single_client_single_echo(echo_backend_server, server_container, client_container) -> None:
    message_to_send = 'bla'
    reader, writer = await asyncio.open_connection('127.0.0.1', TUNNELER_PORT)

    writer.write(message_to_send.encode())
    await writer.drain()
    data = await reader.read(1024)
    received_message = data.decode()
    writer.close()
    await writer.wait_closed()

    assert received_message == message_to_send


@pytest.mark.asyncio
async def test_dns_tunnel_single_client_multiple_echo(echo_backend_server, server_container, client_container) -> None:
    messages_to_send = ['bla', 'bli', 'blu']
    reader, writer = await asyncio.open_connection('127.0.0.1', TUNNELER_PORT)

    received_messages = []
    for message in messages_to_send:
        writer.write(message.encode())
        await writer.drain()
        data = await reader.read(1024)
        received_messages.append(data.decode())

    writer.close()
    await writer.wait_closed()

    assert received_messages == messages_to_send
