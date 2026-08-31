// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Moving Explorer windows off a folder so it can be renamed.
    /// </summary>
    /// <remarks>
    /// <para>An Explorer window showing a folder holds a handle on it, for change
    /// notifications, and that is enough to stop the folder being moved. Killing
    /// explorer.exe would release it and take the taskbar and desktop with it, which is
    /// not a trade worth making to run a test.</para>
    ///
    /// <para>The shell exposes its open windows through IShellWindows, each one an
    /// IWebBrowser2 with a location and the ability to go somewhere else. So the window is
    /// NAVIGATED to the parent folder rather than closed: the handle is released, the
    /// window stays open where the person left it, and Back undoes it. Quit is available
    /// on the same interface and is deliberately not used - closing somebody's window is a
    /// bigger liberty than moving it up one level.</para>
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
            OnShellWindows(folder, (_, location) => found.Add(location), navigate: false);
            return found.ToArray();
        }

        /// <summary>
        /// Points any Explorer window showing a folder at that folder's parent.
        /// </summary>
        /// <param name="folder">The folder to clear.</param>
        /// <param name="announce">Called for each window moved.</param>
        /// <returns>How many windows were moved.</returns>
        public static int NavigateAwayFrom(string folder, Action<string>? announce = null)
        {
            int moved = 0;
            OnShellWindows(
                folder,
                (window, location) =>
                {
                    string? parent = Path.GetDirectoryName(
                        folder.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
                    if (string.IsNullOrEmpty(parent))
                    {
                        return;
                    }

                    try
                    {
                        window.GetType().InvokeMember(
                            "Navigate",
                            BindingFlags.InvokeMethod,
                            null,
                            window,
                            new object[] { parent! });
                        moved++;
                        announce?.Invoke(
                            $"moved an Explorer window from {location} up to {parent}");
                    }
                    catch (Exception)
                    {
                        // The window went away, or refused. Nothing to do about it.
                    }
                },
                navigate: true);

            if (moved > 0)
            {
                // Navigation is asynchronous; the handle is not released the instant the
                // call returns.
                Thread.Sleep(500);
            }

            return moved;
        }

        private static void OnShellWindows(
            string folder, Action<object, string> visit, bool navigate)
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
