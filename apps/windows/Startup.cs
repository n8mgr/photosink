using Microsoft.Win32;
using System;

namespace Photosink;

internal static class Startup
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "Photosink";

    internal static bool Enabled
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey);
            return key?.GetValue(ValueName) is string command && command == Command;
        }
        set
        {
            using var key = Registry.CurrentUser.CreateSubKey(RunKey);
            if (value)
            {
                key.SetValue(ValueName, Command, RegistryValueKind.String);
            }
            else
            {
                key.DeleteValue(ValueName, false);
            }
        }
    }

    private static string Command => $"\"{Environment.ProcessPath ?? throw new InvalidOperationException("Could not locate Photosink.")}\"";
}
