import pytest

from e2e_tests.conftest import TestPorts, run_tunneler_container, TunnelerType, TunneledType, \
    run_test_tcp_single_client_single_short_echo, run_test_tcp_single_client_multiple_short_echo, \
    run_test_tcp_single_client_single_long_echo, run_test_tcp_multiple_clients_single_short_echo, \
    run_test_tcp_multiple_tunnels_single_short_echo, run_test_tcp_server_long_response_and_empty_acks, \
    run_test_udp_single_client_single_short_echo, run_test_udp_single_client_multiple_short_echo, \
    run_test_udp_single_client_single_long_echo, run_test_udp_multiple_tunnels_single_short_echo


@pytest.mark.asyncio
async def test_tcp_single_client_single_short_echo(tcp_echo_server, tcp_over_tcp_server, tcp_over_tcp_client) -> None:
    await run_test_tcp_single_client_single_short_echo()


@pytest.mark.asyncio
async def test_tcp_single_client_multiple_short_echo(tcp_echo_server, tcp_over_tcp_server, tcp_over_tcp_client) -> None:
    await run_test_tcp_single_client_multiple_short_echo()


@pytest.mark.asyncio
async def test_tcp_single_client_single_long_echo(tcp_echo_server, tcp_over_tcp_server, tcp_over_tcp_client) -> None:
    await run_test_tcp_single_client_single_long_echo()


@pytest.mark.asyncio
async def test_tcp_multiple_clients_single_short_echo(tcp_echo_server, tcp_over_tcp_server, tcp_over_tcp_client) -> None:
    await run_test_tcp_multiple_clients_single_short_echo()


@pytest.mark.asyncio
async def test_tcp_multiple_tunnels_single_short_echo(tcp_echo_server, tcp_over_tcp_server, client_image, tcp_over_tcp_client) -> None:
    container = run_tunneler_container(client_image,
                                       'test_another_client',
                                       TunnelerType.TCP,
                                       TunneledType.TCP,
                                       TestPorts.TUNNELER_PORT.value + 1,
                                       TestPorts.UNTUNNELER_PORT)
    await run_test_tcp_multiple_tunnels_single_short_echo(container)


@pytest.mark.asyncio
async def test_tcp_server_long_response_and_empty_acks(redis_server, tcp_over_tcp_server, tcp_over_tcp_client) -> None:
    await run_test_tcp_server_long_response_and_empty_acks()


@pytest.mark.asyncio
async def test_udp_single_client_single_short_echo(udp_echo_server, udp_over_tcp_server, udp_over_tcp_client) -> None:
    await run_test_udp_single_client_single_short_echo()


@pytest.mark.asyncio
async def test_udp_single_client_multiple_short_echo(udp_echo_server, udp_over_tcp_server, udp_over_tcp_client) -> None:
    await run_test_udp_single_client_multiple_short_echo()


@pytest.mark.asyncio
async def test_udp_single_client_single_long_echo(udp_echo_server, udp_over_tcp_server, udp_over_tcp_client) -> None:
    await run_test_udp_single_client_single_long_echo(10_000)


@pytest.mark.asyncio
async def test_udp_multiple_tunnels_single_short_echo(udp_echo_server, udp_over_tcp_server, client_image, udp_over_tcp_client) -> None:
    container = run_tunneler_container(client_image,
                                       'test_another_client',
                                       TunnelerType.TCP,
                                       TunneledType.UDP,
                                       TestPorts.TUNNELER_PORT.value + 1,
                                       TestPorts.UNTUNNELER_PORT)
    await run_test_udp_multiple_tunnels_single_short_echo(container)
