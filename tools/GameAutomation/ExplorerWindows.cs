// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Closing Explorer windows that are holding a folder open.
    /// </summary>
    /// <remarks>
    /// <para>An Explorer window showing a folder holds handles on it - and on its parent
    /// and siblings, from the navigation pane - which is enough to stop the folder being
    /// renamed. Killing explorer.exe would release them and take the taskbar and desktop
    /// with it, which is not a trade worth making to run a test.</para>
    ///
    /// <para>The shell exposes its open windows through IShellWindows, so the windows on
    /// one folder can be closed without touching anything else. Only closing releases the
    /// handles; pointing a window somewhere else leaves them held. Closing loses the
    /// window, which is why the caller keeps this behind an explicit opt-in.</para>
    ///
    /// <para>Through reflection rather than an interop assembly, to avoid taking a
    /// dependency on SHDocVw for three calls, and on a dedicated STA thread because the
    /// shell's COM objects expect one.</para>
    /// </remarks>
    public static class ExplorerWindows
    {
        private static readonly Guid ShellWindowsClsid =
            new Guid("9BA05972-F6A8-11CF-A442-00A0C90A8F39");

        /// <summary>Which open Explorer windows are showing a folder, or something in it.</summary>
        /// <param name="folder">The folder to look for.</param>
        /// <returns>The locations found, one per window.</returns>
        public static string[] Showing(string folder)
        {
            var found = new List<string>();
            OnShellWindows(folder, (_, location) => found.Add(location));
            return found.ToArray();
        }

        /// <summary>
        /// Closes any Explorer window showing a folder, or anything inside it.
        /// </summary>
        /// <param name="folder">The folder to clear.</param>
        /// <param name="announce">Called for each window closed.</param>
        /// <returns>How many windows were closed.</returns>
        public static int CloseShowing(string folder, Action<string>? announce = null)
        {
            int closed = 0;
            OnShellWindows(
                folder,
                (window, location) =>
                {
                    try
                    {
                        window.GetType().InvokeMember(
                            "Quit", BindingFlags.InvokeMethod, null, window, null);
                        closed++;
                        announce?.Invoke($"closed an Explorer window showing {location}");
                    }
                    catch (Exception)
                    {
                        // The window went away, or refused. Nothing to do about it.
                    }
                });

            if (closed > 0)
            {
                // Closing is asynchronous; the handles are not gone the instant Quit
                // returns.
                Thread.Sleep(500);
            }

            return closed;
        }

        private static void OnShellWindows(string folder, Action<object, string> visit)
        {
            string target = Path.GetFullPath(folder).TrimEnd(
                Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);

            // The shell's COM objects want a single-threaded apartment, and a console app
            // is MTA unless told otherwise.
            Exception? failure = null;
            var thread = new Thread(() =>
            {
                try
                {
                    Visit(target, visit);
                }
                catch (Exception error)
                {
                    failure = error;
                }
            });

            thread.SetApartmentState(ApartmentState.STA);
            thread.Start();
            thread.Join(TimeSpan.FromSeconds(10));

            if (failure != null)
            {
                throw new InvalidOperationException(
                    $"Could not ask the shell about its windows: {failure.Message}", failure);
            }
        }

        private static void Visit(string target, Action<object, string> visit)
        {
            Type? type = Type.GetTypeFromCLSID(ShellWindowsClsid);
            if (type == null)
            {
                return;
            }

            object? shell = Activator.CreateInstance(type);
            if (shell == null)
            {
                return;
            }

            int count = (int)shell.GetType().InvokeMember(
                "Count", BindingFlags.GetProperty, null, shell, null)!;

            // Backwards, because acting on a window can remove it from the collection and
            // shift everything after it.
            for (int i = count - 1; i >= 0; i--)
            {
                object? window = null;
                try
                {
                    window = shell.GetType().InvokeMember(
                        "Item", BindingFlags.InvokeMethod, null, shell, new object[] { i });
                }
                catch (Exception)
                {
                    continue;
                }

                if (window == null)
                {
                    continue;
                }

                string? location = LocationOf(window);
                if (location == null)
                {
                    continue;
                }

                if (location.Equals(target, StringComparison.OrdinalIgnoreCase)
                    || location.StartsWith(target + Path.DirectorySeparatorChar,
                        StringComparison.OrdinalIgnoreCase))
                {
                    visit(window, location);
                }
            }
        }

        private static string? LocationOf(object window)
        {
            try
            {
                var url = window.GetType().InvokeMember(
                    "LocationURL", BindingFlags.GetProperty, null, window, null) as string;

                if (string.IsNullOrEmpty(url))
                {
                    // A shell window with no filesystem location - This PC, Recycle Bin,
                    // or an Internet Explorer window sharing the same collection.
                    return null;
                }

                var uri = new Uri(url!);
                if (!uri.IsFile)
                {
                    return null;
                }

                return uri.LocalPath.TrimEnd(
                    Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
            }
            catch (Exception)
            {
                return null;
            }
        }
    }
}
