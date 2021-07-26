import asyncio

import pytest

from e2e_tests.conftest import TestPorts, run_tunneler_container


@pytest.mark.asyncio
async def test_single_client_single_short_echo(echo_backend_server, tcp_server_container, tcp_client_container) -> None:
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
async def test_single_client_multiple_short_echo(echo_backend_server, tcp_server_container, tcp_client_container) -> None:
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
async def test_single_client_single_long_echo(echo_backend_server, tcp_server_container, tcp_client_container) -> None:
    message_to_send = 'bla' * 10_000
    reader, writer = await asyncio.open_connection('127.0.0.1', TestPorts.TUNNELER_PORT.value)

    writer.write(message_to_send.encode())
    await writer.drain()
    data = await reader.readexactly(len(message_to_send))
    received_message = data.decode()
    writer.close()
    await writer.wait_closed()

    assert received_message == message_to_send


@pytest.mark.asyncio
async def test_multiple_clients_single_short_echo(echo_backend_server, tcp_server_container, tcp_client_container) -> None:
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


@pytest.mark.asyncio
async def test_multiple_tunnels_single_short_echo(echo_backend_server, tcp_server_container, client_image, tcp_client_container) -> None:
    container = run_tunneler_container(client_image,
                                       'test_another_client',
                                       'Tcp',
                                       TestPorts.TUNNELER_PORT.value + 1,
                                       TestPorts.UNTUNNELER_PORT)
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
        print(f'another client {container.logs()=}')
        container.kill()
        container.remove(force=True)
