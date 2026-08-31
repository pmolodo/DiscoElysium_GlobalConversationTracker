// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Starting and stopping the Steam client itself.</summary>
    /// <remarks>
    /// <para>Needed because Steam holds its local config in memory and rewrites it on
    /// exit, so any edit made while it is running is discarded. Changing a Steam setting
    /// from outside means stopping Steam first.</para>
    ///
    /// <para>Shutdown goes through <c>steam.exe -shutdown</c> and then WAITS for the
    /// process to disappear. The flag is widely used but not in Valve's documented option
    /// list, so it is treated as best-effort and verified rather than trusted. Nothing
    /// here ever kills the process: a half-written config is the exact failure being
    /// avoided, and force-killing Steam is a good way to cause one.</para>
    /// </remarks>
    public static class SteamClient
    {
        /// <summary>The process name, without .exe.</summary>
        public const string ProcessName = "steam";

        private static readonly string[] DefaultPaths =
        {
            @"C:\apps (x86)\games\steam\steam.exe",
            @"C:\Program Files (x86)\Steam\steam.exe",
            @"C:\Program Files\Steam\steam.exe",
        };

        /// <summary>Whether the Steam client is running.</summary>
        public static bool IsRunning => Process.GetProcessesByName(ProcessName).Length > 0;

        /// <summary>Finds steam.exe.</summary>
        /// <param name="hint">A known path to use instead of searching.</param>
        /// <exception cref="FileNotFoundException">It could not be found.</exception>
        public static string FindExecutable(string? hint = null)
        {
            if (!string.IsNullOrEmpty(hint))
            {
                if (!File.Exists(hint))
                {
                    throw new FileNotFoundException($"No Steam client at {hint}.", hint);
                }

                return hint!;
            }

            // A running client knows where it lives, which beats guessing.
            foreach (Process process in Process.GetProcessesByName(ProcessName))
            {
                try
                {
                    string? path = process.MainModule?.FileName;
                    if (!string.IsNullOrEmpty(path) && File.Exists(path))
                    {
                        return path!;
                    }
                }
                catch (Exception)
                {
                    // A 32-bit/64-bit or permission mismatch; fall through to the list.
                }
            }

            foreach (string candidate in DefaultPaths)
            {
                if (File.Exists(candidate))
                {
                    return candidate;
                }
            }

            throw new FileNotFoundException(
                "Could not find steam.exe. Pass its path explicitly.");
        }

        /// <summary>Asks Steam to exit, and waits until it has.</summary>
        /// <param name="executable">Path to steam.exe.</param>
        /// <param name="timeout">How long to wait for it to go.</param>
        /// <param name="progress">Called with status, for verbose output.</param>
        /// <exception cref="TimeoutException">Steam was still running at the deadline.</exception>
        public static void Shutdown(
            string executable, TimeSpan timeout, Action<string>? progress = null)
        {
            if (!IsRunning)
            {
                progress?.Invoke("Steam is not running");
                return;
            }

            progress?.Invoke("asking Steam to exit...");
            using (var process = Process.Start(new ProcessStartInfo(executable, "-shutdown")
            {
                UseShellExecute = false,
            }))
            {
                process?.WaitForExit((int)timeout.TotalMilliseconds);
            }

            DateTime deadline = DateTime.UtcNow + timeout;
            while (DateTime.UtcNow < deadline)
            {
                if (!IsRunning)
                {
                    progress?.Invoke("Steam has exited");
                    return;
                }

                Thread.Sleep(500);
            }

            throw new TimeoutException(
                $"Steam was still running {timeout.TotalSeconds:N0}s after -shutdown. Not killing "
                + "it: a half-written localconfig.vdf is exactly the damage this avoids. Close "
                + "Steam by hand and try again.");
        }

        /// <summary>Starts Steam and waits for it to come up.</summary>
        /// <param name="executable">Path to steam.exe.</param>
        /// <param name="timeout">How long to wait.</param>
        /// <param name="silent">Start minimised to the tray.</param>
        /// <param name="progress">Called with status, for verbose output.</param>
        /// <exception cref="TimeoutException">It never appeared.</exception>
        public static void Start(
            string executable,
            TimeSpan timeout,
            bool silent = true,
            Action<string>? progress = null)
        {
            if (IsRunning)
            {
                progress?.Invoke("Steam is already running");
                return;
            }

            progress?.Invoke("starting Steam...");
            Process.Start(new ProcessStartInfo(executable, silent ? "-silent" : string.Empty)
            {
                UseShellExecute = false,
            });

            DateTime deadline = DateTime.UtcNow + timeout;
            while (DateTime.UtcNow < deadline)
            {
                if (IsRunning)
                {
                    progress?.Invoke("Steam is up");
                    return;
                }

                Thread.Sleep(500);
            }

            throw new TimeoutException(
                $"Steam did not start within {timeout.TotalSeconds:N0}s.");
        }
    }
}
