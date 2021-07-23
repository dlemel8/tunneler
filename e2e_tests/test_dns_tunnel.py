import asyncio

import pytest

from e2e_tests.conftest import TestPorts


@pytest.mark.asyncio
async def test_single_client_single_short_echo(echo_backend_server, server_container, client_container) -> None:
    message_to_send = 'bla'
    reader, writer = await asyncio.open_connection('127.0.0.1', TestPorts.TUNNELER_PORT.value)

    writer.write(message_to_send.encode())
    await writer.drain()
    data = await reader.readexactly(len(message_to_send))
    received_message = data.decode()
    writer.close()
    await writer.wait_closed()

    assert received_message == message_to_send


@pytest.mark.asyncio
async def test_single_client_multiple_short_echo(echo_backend_server, server_container, client_container) -> None:
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


@pytest.mark.asyncio
async def test_single_client_single_long_echo(echo_backend_server, server_container, client_container) -> None:
    message_to_send = 'bla' * 10_000
    reader, writer = await asyncio.open_connection('127.0.0.1', TestPorts.TUNNELER_PORT.value)

    writer.write(message_to_send.encode())
    await writer.drain()
    data = await reader.readexactly(len(message_to_send))
    received_message = data.decode()
    writer.close()
    await writer.wait_closed()

    assert received_message == message_to_send
