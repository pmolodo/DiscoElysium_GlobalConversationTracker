// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

namespace GlobalConversationTracker.Automation
{
    /// <summary>A process holding something open.</summary>
    public sealed class LockHolder
    {
        /// <summary>Creates a record.</summary>
        /// <param name="processId">The process id.</param>
        /// <param name="name">Its application name, as the Restart Manager reports it.</param>
        /// <param name="window">Its main window title, or empty if it has none.</param>
        public LockHolder(int processId, string name, string window)
        {
            ProcessId = processId;
            Name = name;
            Window = window;
        }

        /// <summary>The process id.</summary>
        public int ProcessId { get; }

        /// <summary>The application name.</summary>
        public string Name { get; }

        /// <summary>Its main window title, or empty.</summary>
        public string Window { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return Window.Length > 0
                ? $"{Name} (pid {ProcessId}) - \"{Window}\""
                : $"{Name} (pid {ProcessId})";
        }
    }

    /// <summary>
    /// Finding out which processes are holding a file or folder open.
    /// </summary>
    /// <remarks>
    /// <para>Windows reports a failed move as a bare "the process cannot access the file
    /// because it is being used by another process", which does not say WHICH process, so
    /// the person reading it has to go hunting. The Restart Manager - the service
    /// installers use to work out what to close before an update - will answer that, and
    /// is the documented way to ask.</para>
    ///
    /// <para>It reports holders of FILES. A folder someone merely has open in Explorer,
    /// or a working directory an editor is sitting in, may hold no file handle at all and
    /// so appear in no list while still blocking a directory move. An empty answer
    /// therefore means "no file handles found", never "nothing is holding it".</para>
    /// </remarks>
    public static class FileLocks
    {
        private const int RmRebootReasonNone = 0;
        private const int CchRmMaxAppName = 255;
        private const int CchRmMaxSvcName = 63;
        private const int ErrorMoreData = 234;

        /// <summary>How many files inside a folder to ask about.</summary>
        /// <remarks>
        /// The Restart Manager takes a list of files, and a save folder can hold hundreds.
        /// Asking about a sample is enough to name the culprit, and asking about all of
        /// them is slow for no extra answer.
        /// </remarks>
        public const int MaxFilesInspected = 64;

        /// <summary>Which processes hold a file or anything inside a folder.</summary>
        /// <param name="path">The file or folder to ask about.</param>
        /// <returns>What was found; empty if nothing was, which is not proof of nothing.</returns>
        public static LockHolder[] WhoIsUsing(string path)
        {
            var targets = new List<string>();
            if (Directory.Exists(path))
            {
                targets.Add(path);
                try
                {
                    foreach (string file in Directory.EnumerateFiles(
                        path, "*", SearchOption.AllDirectories))
                    {
                        targets.Add(file);
                        if (targets.Count > MaxFilesInspected)
                        {
                            break;
                        }
                    }
                }
                catch (Exception)
                {
                    // Cannot enumerate it - which may itself be why the move failed. What
                    // was collected so far is still worth asking about.
                }
            }
            else if (File.Exists(path))
            {
                targets.Add(path);
            }

            return targets.Count == 0 ? Array.Empty<LockHolder>() : Ask(targets.ToArray());
        }

        /// <summary>One line naming the holders, for an error message.</summary>
        /// <param name="path">The file or folder to ask about.</param>
        public static string Describe(string path)
        {
            LockHolder[] holders;
            try
            {
                holders = WhoIsUsing(path);
            }
            catch (Exception error)
            {
                return $"(could not work out what is holding it: {error.Message})";
            }

            if (holders.Length == 0)
            {
                // Explorer sitting in the folder, or an editor with it as a working
                // directory, holds no file handle and shows up in no list.
                return "No process holds a file handle inside it. Something may still have "
                    + "the FOLDER open - Explorer showing it, or an editor working in it - "
                    + "which blocks a move without holding any file.";
            }

            var names = new List<string>();
            foreach (LockHolder holder in holders)
            {
                names.Add(holder.ToString());
            }

            return "Held by: " + string.Join("; ", names);
        }

        private static LockHolder[] Ask(string[] files)
        {
            int result = RmStartSession(out uint session, 0, Guid.NewGuid().ToString("N"));
            if (result != 0)
            {
                throw new InvalidOperationException(
                    $"Could not start a Restart Manager session ({result}).");
            }

            try
            {
                result = RmRegisterResources(
                    session, (uint)files.Length, files, 0, null, 0, null);
                if (result != 0)
                {
                    throw new InvalidOperationException(
                        $"Could not register {files.Length} paths with the Restart Manager ({result}).");
                }

                uint needed = 0;
                uint have = 0;
                uint reason = RmRebootReasonNone;

                result = RmGetList(session, out needed, ref have, null, ref reason);
                if (result == ErrorMoreData)
                {
                    var info = new RmProcessInfo[needed];
                    have = needed;
                    result = RmGetList(session, out needed, ref have, info, ref reason);
                    if (result != 0)
                    {
                        throw new InvalidOperationException(
                            $"Could not read the Restart Manager's list ({result}).");
                    }

                    var holders = new List<LockHolder>();
                    for (int i = 0; i < have; i++)
                    {
                        int pid = info[i].Process.ProcessId;
                        string window = string.Empty;
                        try
                        {
                            using var process = System.Diagnostics.Process.GetProcessById(pid);
                            window = process.MainWindowTitle ?? string.Empty;
                        }
                        catch (Exception)
                        {
                            // It exited between the list and here, or is not ours to look at.
                        }

                        holders.Add(new LockHolder(pid, info[i].strAppName, window));
                    }

                    return holders.ToArray();
                }

                if (result != 0)
                {
                    throw new InvalidOperationException(
                        $"Could not read the Restart Manager's list ({result}).");
                }

                return Array.Empty<LockHolder>();
            }
            finally
            {
                RmEndSession(session);
            }
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct RmUniqueProcess
        {
            public int ProcessId;
            public System.Runtime.InteropServices.ComTypes.FILETIME ProcessStartTime;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct RmProcessInfo
        {
            public RmUniqueProcess Process;

            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = CchRmMaxAppName + 1)]
            public string strAppName;

            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = CchRmMaxSvcName + 1)]
            public string strServiceShortName;

            public int ApplicationType;
            public uint AppStatus;
            public uint TSSessionId;

            [MarshalAs(UnmanagedType.Bool)]
            public bool bRestartable;
        }

        [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
        private static extern int RmStartSession(
            out uint pSessionHandle, int dwSessionFlags, string strSessionKey);

        [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
        private static extern int RmRegisterResources(
            uint pSessionHandle,
            uint nFiles,
            string[] rgsFilenames,
            uint nApplications,
            RmUniqueProcess[]? rgApplications,
            uint nServices,
            string[]? rgsServiceNames);

        [DllImport("rstrtmgr.dll")]
        private static extern int RmGetList(
            uint dwSessionHandle,
            out uint pnProcInfoNeeded,
            ref uint pnProcInfo,
            [In, Out] RmProcessInfo[]? rgAffectedApps,
            ref uint lpdwRebootReasons);

        [DllImport("rstrtmgr.dll")]
        private static extern int RmEndSession(uint pSessionHandle);
    }
}
