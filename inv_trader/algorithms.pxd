#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="algorithms.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.model.objects cimport Price, Symbol, Tick, BarType, Bar
from inv_trader.model.order cimport Order


cdef class TrailingStopSignal:
    """
    Represents a trailing stop signal.
    """
    cdef bint is_signal
    cdef Price price


cdef class TrailingStopAlgorithm:
    """
    The base class for all trailing stop algorithms.
    """
    cdef Order order

    cdef object _calculate
    cdef object generate

    cdef TrailingStopSignal _generate_buy(self, Price update_price)
    cdef TrailingStopSignal _generate_sell(self, Price update_price)


cdef class TickTrailingStopAlgorithm(TrailingStopAlgorithm):
    """
    The base class for all trailing stop algorithms.
    """
    cdef readonly Symbol symbol

    cpdef void update(self, Tick tick)
    cpdef TrailingStopSignal calculate_buy(self, Tick tick)
    cpdef TrailingStopSignal calculate_sell(self, Tick tick)


cdef class BarTrailingStopAlgorithm(TrailingStopAlgorithm):
    """
    The base class for all trailing stop algorithms updated with bars.
    """
    cdef readonly BarType bar_type

    cpdef void update(self, Bar bar)
    cpdef TrailingStopSignal calculate_buy(self, Bar bar)
    cpdef TrailingStopSignal calculate_sell(self, Bar bar)


cdef class BarsBackTrail(BarTrailingStopAlgorithm):
    """
    A trailing stop algorithm based on the number of bars back.
    """
    cdef int _bars_back
    cdef float _sl_atr_multiple
    cdef list _bars
    cdef object _atr
