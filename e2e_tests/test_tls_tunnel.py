import pytest

from e2e_tests.conftest import run_test_tcp_single_client_single_short_echo, \
    run_test_tcp_single_client_multiple_short_echo, run_test_tcp_single_client_single_long_echo, \
    run_test_tcp_multiple_clients_single_short_echo, run_tunneler_container, TunnelerType, TunneledType, TestPorts, \
    TLS_SERVER_NAME, run_test_tcp_multiple_tunnels_single_short_echo, \
    run_test_tcp_server_long_response_and_empty_acks, run_test_udp_single_client_single_short_echo, \
    run_test_udp_single_client_multiple_short_echo, run_test_udp_single_client_single_long_echo, \
    run_test_udp_multiple_tunnels_single_short_echo


@pytest.mark.asyncio
async def test_tcp_single_client_single_short_echo(tcp_echo_server, tcp_over_tls_server, tcp_over_tls_client) -> None:
    await run_test_tcp_single_client_single_short_echo()


@pytest.mark.asyncio
async def test_tcp_single_client_multiple_short_echo(tcp_echo_server, tcp_over_tls_server, tcp_over_tls_client) -> None:
    await run_test_tcp_single_client_multiple_short_echo()


@pytest.mark.asyncio
async def test_tcp_single_client_single_long_echo(tcp_echo_server, tcp_over_tls_server, tcp_over_tls_client) -> None:
    await run_test_tcp_single_client_single_long_echo()


@pytest.mark.asyncio
async def test_tcp_multiple_clients_single_short_echo(tcp_echo_server, tcp_over_tls_server,
                                                      tcp_over_tls_client) -> None:
    await run_test_tcp_multiple_clients_single_short_echo()


@pytest.mark.asyncio
async def test_tcp_multiple_tunnels_single_short_echo(tcp_echo_server, tcp_over_tls_server, client_image,
                                                      tcp_over_tls_client, pki) -> None:
    container = run_tunneler_container(client_image,
                                       'test_another_client',
                                       TunnelerType.TLS,
                                       TunneledType.TCP,
                                       TestPorts.TUNNELER_PORT.value + 1,
                                       TestPorts.UNTUNNELER_PORT,
                                       extra_env_vars={'CA_CERT': '/ca_certificate',
                                                       'KEY': '/client_key',
                                                       'CERT': '/client_certificate',
                                                       'SERVER_HOSTNAME': TLS_SERVER_NAME},
                                       volumes=[(str(pki.ca_certificate), '/ca_certificate'),
                                                (str(pki.client_key), '/client_key'),
                                                (str(pki.client_certificate), '/client_certificate')])
    await run_test_tcp_multiple_tunnels_single_short_echo(container)


@pytest.mark.asyncio
async def test_tcp_server_long_response_and_empty_acks(redis_server, tcp_over_tls_server, tcp_over_tls_client) -> None:
    await run_test_tcp_server_long_response_and_empty_acks()


@pytest.mark.asyncio
async def test_udp_single_client_single_short_echo(udp_echo_server, udp_over_tls_server, udp_over_tls_client) -> None:
    await run_test_udp_single_client_single_short_echo()


@pytest.mark.asyncio
async def test_udp_single_client_multiple_short_echo(udp_echo_server, udp_over_tls_server, udp_over_tls_client) -> None:
    await run_test_udp_single_client_multiple_short_echo()


@pytest.mark.asyncio
async def test_udp_single_client_single_long_echo(udp_echo_server, udp_over_tls_server, udp_over_tls_client) -> None:
    await run_test_udp_single_client_single_long_echo(10_000)


@pytest.mark.asyncio
async def test_udp_multiple_tunnels_single_short_echo(udp_echo_server, udp_over_tls_server, client_image,
                                                      udp_over_tls_client, pki) -> None:
    container = run_tunneler_container(client_image,
                                       'test_another_client',
                                       TunnelerType.TLS,
                                       TunneledType.UDP,
                                       TestPorts.TUNNELER_PORT.value + 1,
                                       TestPorts.UNTUNNELER_PORT,
                                       extra_env_vars={'CA_CERT': '/ca_certificate',
                                                       'KEY': '/client_key',
                                                       'CERT': '/client_certificate',
                                                       'SERVER_HOSTNAME': TLS_SERVER_NAME},
                                       volumes=[(str(pki.ca_certificate), '/ca_certificate'),
                                                (str(pki.client_key), '/client_key'),
                                                (str(pki.client_certificate), '/client_certificate')])
    await run_test_udp_multiple_tunnels_single_short_echo(container)
