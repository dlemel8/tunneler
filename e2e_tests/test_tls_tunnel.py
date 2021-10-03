import pytest

from e2e_tests.conftest import run_test_tcp_single_client_single_short_echo


@pytest.mark.asyncio
async def test_tcp_single_client_single_short_echo(tcp_echo_server, tcp_over_tls_server, tcp_over_tls_client) -> None:
    await run_test_tcp_single_client_single_short_echo()

