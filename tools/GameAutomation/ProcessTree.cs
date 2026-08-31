// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Walking up from a process to the one with the window.
    /// </summary>
    /// <remarks>
    /// <para>Modern editors and browsers are a tree: one process owns the window and a
    /// dozen children do the work. The child holding a folder open is usually a file
    /// watcher with no window at all, so asking IT to close does nothing - the request is
    /// a window message and there is no window to send it to.</para>
    ///
    /// <para>Measured on a real machine: sixteen Code.exe processes, fifteen of them
    /// windowless children of the one that owns the editor window, and the folder was held
    /// by a utility child.</para>
    /// </remarks>
    public static class ProcessTree
    {
        private const uint Th32csSnapProcess = 0x00000002;

        /// <summary>How far up to look before giving up.</summary>
        /// <remarks>
        /// A child-to-window hop is normally one or two. A bound is needed anyway, because
        /// recycled process ids can make the parent chain appear to loop.
        /// </remarks>
        public const int MaxDepth = 8;

        /// <summary>The parent of a process, or 0 if it cannot be found.</summary>
        /// <param name="processId">The process to look up.</param>
        public static int ParentOf(int processId)
        {
            IntPtr snapshot = CreateToolhelp32Snapshot(Th32csSnapProcess, 0);
            if (snapshot == IntPtr.Zero || snapshot == new IntPtr(-1))
            {
                return 0;
            }

            try
            {
                var entry = new ProcessEntry32
                {
                    Size = Marshal.SizeOf(typeof(ProcessEntry32)),
                };

                if (!Process32First(snapshot, ref entry))
                {
                    return 0;
                }

                do
                {
                    if (entry.ProcessId == processId)
                    {
                        return (int)entry.ParentProcessId;
                    }
                }
                while (Process32Next(snapshot, ref entry));

                return 0;
            }
            finally
            {
                CloseHandle(snapshot);
            }
        }

        /// <summary>
        /// The nearest process at or above this one that has a window.
        /// </summary>
        /// <remarks>
        /// Ancestors are checked to have started BEFORE the process they are the parent
        /// of. Process ids are reused, so an unrelated process can inherit the id of a
        /// long-dead parent, and closing it would be closing something at random.
        /// </remarks>
        /// <param name="processId">Where to start.</param>
        /// <param name="isAcceptable">
        /// Whether a candidate's process name is one the caller is willing to act on.
        /// Checked at every level, so walking up cannot arrive somewhere unintended.
        /// </param>
        /// <returns>The process id, or 0 if nothing suitable was found.</returns>
        public static int NearestWithWindow(int processId, Func<string, bool> isAcceptable)
        {
            if (isAcceptable == null)
            {
                throw new ArgumentNullException(nameof(isAcceptable));
            }

            int current = processId;
            DateTime childStarted = DateTime.MaxValue;

            for (int depth = 0; depth < MaxDepth && current > 4; depth++)
            {
                Process? process = null;
                try
                {
                    process = Process.GetProcessById(current);
                }
                catch (Exception)
                {
                    return 0;
                }

                using (process)
                {
                    DateTime started;
                    try
                    {
                        started = process.StartTime;
                    }
                    catch (Exception)
                    {
                        return 0;
                    }

                    // A parent cannot have started after its child. If it did, this id has
                    // been reused and the chain is fiction from here up.
                    if (started > childStarted)
                    {
                        return 0;
                    }

                    if (!isAcceptable(process.ProcessName))
                    {
                        return 0;
                    }

                    if (process.MainWindowHandle != IntPtr.Zero)
                    {
                        return current;
                    }

                    childStarted = started;
                }

                current = ParentOf(current);
            }

            return 0;
        }

        /// <summary>Every process of a given name that has a window.</summary>
        /// <remarks>
        /// The fallback for when the tree cannot be walked - a parent that has already
        /// exited, or a holder whose window-owning ancestor is not its parent. Closing all
        /// of an application's windows is blunter than closing the one that matters, and
        /// it is the only thing left that works.
        /// </remarks>
        /// <param name="processName">The process name, without .exe.</param>
        public static int[] WithWindowsNamed(string processName)
        {
            var found = new List<int>();
            foreach (Process process in Process.GetProcessesByName(processName))
            {
                using (process)
                {
                    try
                    {
                        if (process.MainWindowHandle != IntPtr.Zero)
                        {
                            found.Add(process.Id);
                        }
                    }
                    catch (Exception)
                    {
                        // Exited, or not ours to inspect.
                    }
                }
            }

            return found.ToArray();
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ProcessEntry32
        {
            public int Size;
            public uint Usage;
            public uint ProcessId;
            public IntPtr DefaultHeapId;
            public uint ModuleId;
            public uint Threads;
            public uint ParentProcessId;
            public int PriorityClassBase;
            public uint Flags;

            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
            public string ExeFile;
        }

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint processId);

        [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
        private static extern bool Process32First(IntPtr snapshot, ref ProcessEntry32 entry);

        [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
        private static extern bool Process32Next(IntPtr snapshot, ref ProcessEntry32 entry);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr handle);
    }
}
