// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Stops and starts Steam without ever force-killing it.</summary>
    public static class SteamClient
    {
        private const string ProcessName = "steam";

        private static readonly string[] DefaultPaths =
        {
            @"C:\apps (x86)\games\steam\steam.exe",
            @"C:\Program Files (x86)\Steam\steam.exe",
            @"C:\Program Files\Steam\steam.exe",
        };

        /// <summary>Whether Steam is running.</summary>
        public static bool IsRunning => Process.GetProcessesByName(ProcessName).Length > 0;

        /// <summary>Finds steam.exe.</summary>
        public static string FindExecutable()
        {
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
                    // Fall through to the known install paths.
                }
                finally
                {
                    process.Dispose();
                }
            }

            foreach (string path in DefaultPaths)
            {
                if (File.Exists(path))
                {
                    return path;
                }
            }

            throw new FileNotFoundException("Could not find steam.exe.");
        }

        /// <summary>Asks Steam to exit and waits for every Steam process to leave.</summary>
        public static void Shutdown(
            string executable, TimeSpan timeout, Action<string>? progress = null)
        {
            if (!IsRunning)
            {
                progress?.Invoke("Steam is already stopped");
                return;
            }

            progress?.Invoke("asking Steam to exit...");
            using (Process? request = Process.Start(new ProcessStartInfo(executable, "-shutdown")
            {
                UseShellExecute = false,
            }))
            {
                request?.WaitForExit((int)timeout.TotalMilliseconds);
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
                $"Steam is still running after {timeout.TotalSeconds:N0}s. It was not killed; "
                + "close it normally and try again.");
        }

        /// <summary>Starts Steam silently and waits for its process.</summary>
        public static void Start(
            string executable, TimeSpan timeout, Action<string>? progress = null)
        {
            if (IsRunning)
            {
                return;
            }

            progress?.Invoke("starting Steam...");
            Process.Start(new ProcessStartInfo(executable, "-silent")
            {
                UseShellExecute = false,
            });

            DateTime deadline = DateTime.UtcNow + timeout;
            while (DateTime.UtcNow < deadline)
            {
                if (IsRunning)
                {
                    progress?.Invoke("Steam is running");
                    return;
                }

                Thread.Sleep(500);
            }

            throw new TimeoutException(
                $"Steam did not start within {timeout.TotalSeconds:N0}s.");
        }
    }
}
