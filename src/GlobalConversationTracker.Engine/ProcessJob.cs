// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// A Windows job object that kills what is in it when this process goes away.
    /// </summary>
    /// <remarks>
    /// <para>THE ORPHAN PROBLEM - de-bnjy.1.1.3. A child started with
    /// <see cref="Process"/> is not tied to its parent by anything: closing the pipe asks
    /// it to leave and it does, but a game that is KILLED rather than closed never gets to
    /// ask. The engine would then sit there holding a fifteen-megabyte index, invisible in
    /// everything but the task list, one copy per time the player force-quit.</para>
    ///
    /// <para>A job object with <c>JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE</c> is the reliable
    /// answer, and reliable is the word that matters: the KERNEL does the killing when the
    /// last handle to the job closes, which happens however the parent ends - a clean exit,
    /// an unhandled exception, a kill from the task manager, a machine that runs out of
    /// memory. No code of ours has to run at the right moment, which is fortunate, because
    /// the moment in question is one where none of our code runs.</para>
    ///
    /// <para>ONE JOB FOR THE PROCESS, created on first use and never closed, so its
    /// lifetime is the game's. Closing it early would kill every engine at once.</para>
    ///
    /// <para>Windows only, and silently nothing everywhere else. There is no job object on
    /// Linux or macOS; the closed pipe is the fallback the parent issue names, and it
    /// covers every ending except a killed parent. The game ships on Windows, so what is
    /// lost is coverage for a development machine.</para>
    /// </remarks>
    internal static class ProcessJob
    {
        /// <summary>Kill everything in the job when its last handle closes.</summary>
        private const int LimitKillOnJobClose = 0x2000;

        /// <summary>The information class for the extended limits structure below.</summary>
        private const int ExtendedLimitInformation = 9;

        private static readonly object Gate = new object();
        private static IntPtr _job = IntPtr.Zero;
        private static bool _tried;

        /// <summary>
        /// Puts <paramref name="child"/> in the job, so it cannot outlive this process.
        /// </summary>
        /// <remarks>
        /// BEST EFFORT, and deliberately silent. An engine that is running but not adopted
        /// is a mod that works, with one failure mode back; an exception here would be a
        /// mod that does not work, over a safety net. The caller has nothing useful to do
        /// about it either way, which is why this returns whether it worked rather than
        /// throwing - for a test to assert on, not for the plugin to branch on.
        /// </remarks>
        /// <returns>Whether the child was adopted.</returns>
        internal static bool Adopt(Process child)
        {
            if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            {
                return false;
            }

            IntPtr job = Job();
            if (job == IntPtr.Zero)
            {
                return false;
            }

            try
            {
                return AssignProcessToJobObject(job, child.Handle);
            }
            catch (Exception)
            {
                // The child may have exited between starting and here, in which case its
                // handle is no longer assignable and there is nothing to adopt.
                return false;
            }
        }

        /// <summary>
        /// The one job, created on first use.
        /// </summary>
        /// <remarks>
        /// Tried ONCE. A machine where this fails will fail every time, and retrying per
        /// child would mean a syscall per look-ahead for an answer already known.
        /// </remarks>
        private static IntPtr Job()
        {
            lock (Gate)
            {
                if (_tried)
                {
                    return _job;
                }

                _tried = true;
                try
                {
                    _job = Create();
                }
                catch (Exception)
                {
                    _job = IntPtr.Zero;
                }

                return _job;
            }
        }

        /// <summary>Creates the job and sets the kill-on-close limit, or returns zero.</summary>
        private static IntPtr Create()
        {
            IntPtr job = CreateJobObject(IntPtr.Zero, null);
            if (job == IntPtr.Zero)
            {
                return IntPtr.Zero;
            }

            var limits = new ExtendedLimitInformationStruct();
            limits.BasicLimitInformation.LimitFlags = LimitKillOnJobClose;

            int size = Marshal.SizeOf<ExtendedLimitInformationStruct>();
            IntPtr buffer = Marshal.AllocHGlobal(size);
            try
            {
                Marshal.StructureToPtr(limits, buffer, fDeleteOld: false);
                if (!SetInformationJobObject(job, ExtendedLimitInformation, buffer, (uint)size))
                {
                    // A job without the limit would adopt children and then not kill them,
                    // which is the appearance of the guarantee without the guarantee.
                    CloseHandle(job);
                    return IntPtr.Zero;
                }
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }

            return job;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateJobObject(IntPtr attributes, string? name);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetInformationJobObject(
            IntPtr job, int informationClass, IntPtr information, uint length);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CloseHandle(IntPtr handle);

        /// <summary>
        /// <c>JOBOBJECT_BASIC_LIMIT_INFORMATION</c>, as Windows lays it out.
        /// </summary>
        /// <remarks>
        /// Every field is here even though one is used, because the LAYOUT is what has to
        /// be right: the kernel reads this by offset, and a struct missing a field would
        /// put the flags somewhere else and set a limit nobody asked for.
        /// </remarks>
        [StructLayout(LayoutKind.Sequential)]
        private struct BasicLimitInformationStruct
        {
            public long PerProcessUserTimeLimit;
            public long PerJobUserTimeLimit;
            public int LimitFlags;
            public UIntPtr MinimumWorkingSetSize;
            public UIntPtr MaximumWorkingSetSize;
            public int ActiveProcessLimit;
            public UIntPtr Affinity;
            public int PriorityClass;
            public int SchedulingClass;
        }

        /// <summary><c>IO_COUNTERS</c>, present for the same reason.</summary>
        [StructLayout(LayoutKind.Sequential)]
        private struct IoCountersStruct
        {
            public ulong ReadOperationCount;
            public ulong WriteOperationCount;
            public ulong OtherOperationCount;
            public ulong ReadTransferCount;
            public ulong WriteTransferCount;
            public ulong OtherTransferCount;
        }

        /// <summary><c>JOBOBJECT_EXTENDED_LIMIT_INFORMATION</c>.</summary>
        [StructLayout(LayoutKind.Sequential)]
        private struct ExtendedLimitInformationStruct
        {
            public BasicLimitInformationStruct BasicLimitInformation;
            public IoCountersStruct IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }
    }
}
