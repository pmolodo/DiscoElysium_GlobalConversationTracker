// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.Harness
{
    /// <summary>Counts what passed and what did not, across a whole run.</summary>
    /// <remarks>
    /// Its own file rather than nested in one run, because more than one run reports this
    /// way: the look-ahead run over its suites, and the evaluate run over the expressions
    /// it asks the game. A second copy would drift from the first in the small ways that
    /// matter when a failure is being read - which line says PASS, how a detail is
    /// indented - and two runs reporting differently is how a reader stops trusting
    /// either.
    /// </remarks>
    internal sealed class Report
    {
        private readonly List<string> _failures = new List<string>();

        /// <summary>How many checks have run.</summary>
        public int Total { get; private set; }

        /// <summary>How many of them passed.</summary>
        public int Passed => Total - _failures.Count;

        /// <summary>What failed, in the order it failed.</summary>
        public IReadOnlyList<string> Failures => _failures;

        /// <summary>Records one check.</summary>
        public void Check(bool ok, string label, string detail)
        {
            Total++;
            Console.WriteLine($"  {(ok ? "PASS" : "FAIL")}  {label}");
            if (detail.Length > 0)
            {
                Console.WriteLine($"        {detail}");
            }

            if (!ok)
            {
                _failures.Add(label);
            }
        }
    }
}
