# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class BetfairDataClientConfig(LiveDataClientConfig, kw_only=True, frozen=True):
    """
    Configuration for ``BetfairDataClient`` instances.

    Parameters
    ----------
    username : str, optional
        The Betfair account username.
    password : str, optional
        The Betfair account password.
    app_key : str, optional
        The betfair application key.
    cert_dir : str, optional
        The local directory that contains the betfair certificates.

    """

    account_currency: str
    username: str | None = None
    password: str | None = None
    app_key: str | None = None
    cert_dir: str | None = None
    instrument_config: BetfairInstrumentProviderConfig | None = None


class BetfairExecClientConfig(LiveExecClientConfig, kw_only=True, frozen=True):
    """
    Configuration for ``BetfairExecClient`` instances.

    Parameters
    ----------
    username : str, optional
        The Betfair account username.
    password : str, optional
        The Betfair account password.
    app_key : str, optional
        The betfair application key.
    cert_dir : str, optional
        The local directory that contains the betfair certificates.

    """

    account_currency: str
    username: str | None = None
    password: str | None = None
    app_key: str | None = None
    cert_dir: str | None = None
    instrument_config: BetfairInstrumentProviderConfig | None = None
    request_account_state_period: int | None = None
