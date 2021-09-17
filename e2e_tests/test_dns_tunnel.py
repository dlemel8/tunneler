import pytest

from e2e_tests.conftest import TestPorts, run_tunneler_container, TunnelerType, DNS_SUFFIX, TunneledType, \
    run_test_tcp_single_client_single_short_echo, run_test_tcp_single_client_multiple_short_echo, \
    run_test_tcp_single_client_single_long_echo, run_test_tcp_multiple_clients_single_short_echo, \
    run_test_tcp_multiple_tunnels_single_short_echo, run_test_tcp_server_long_response_and_empty_acks


@pytest.mark.asyncio
async def test_tcp_single_client_single_short_echo(tcp_echo_server, tcp_over_dns_server, tcp_over_dns_client) -> None:
    await run_test_tcp_single_client_single_short_echo()


@pytest.mark.asyncio
async def test_tcp_single_client_multiple_short_echo(tcp_echo_server, tcp_over_dns_server, tcp_over_dns_client) -> None:
    await run_test_tcp_single_client_multiple_short_echo()


@pytest.mark.asyncio
async def test_tcp_single_client_single_long_echo(tcp_echo_server, tcp_over_dns_server, tcp_over_dns_client) -> None:
    await run_test_tcp_single_client_single_long_echo()


@pytest.mark.asyncio
async def test_tcp_multiple_clients_single_short_echo(tcp_echo_server, tcp_over_dns_server, tcp_over_dns_client) -> None:
    await run_test_tcp_multiple_clients_single_short_echo()


@pytest.mark.asyncio
async def test_tcp_multiple_tunnels_single_short_echo(tcp_echo_server, tcp_over_dns_server, client_image, tcp_over_dns_client) -> None:
    container = run_tunneler_container(client_image,
                                       'test_another_client',
                                       TunnelerType.DNS,
                                       TunneledType.TCP,
                                       TestPorts.TUNNELER_PORT.value + 1,
                                       TestPorts.UNTUNNELER_PORT,
                                       extra_env_vars={'READ_TIMEOUT_IN_MILLISECONDS': 100,
                                                       'IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS': 30000,
                                                       'CLIENT_SUFFIX': DNS_SUFFIX})
    await run_test_tcp_multiple_tunnels_single_short_echo(container)


@pytest.mark.asyncio
async def test_tcp_server_long_response_and_empty_acks(redis_server, tcp_over_dns_server, tcp_over_dns_client) -> None:
    await run_test_tcp_server_long_response_and_empty_acks()
