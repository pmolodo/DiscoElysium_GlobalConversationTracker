# Reading this project's environment variables from PowerShell, with the prefix supplied.
#
# Import it:
#
#     Import-Module "$PSScriptRoot/DegctEnv.psm1"
#     $out = Get-DegctEnv MENUS_OUT -Default 'measurements/logs/somewhere'
#     if (Test-DegctEnv NOLIMIT) { 'limits off' }
#
# WHY A HELPER RATHER THAN A CONVENTION. Every environment variable this project defines is
# prefixed DEGCT_ - see CLAUDE.md for the rule and docs/environment.md for the list - and a
# convention a person has to remember is one that grows exceptions. It cost time three
# separate times before the rule existed, each time because a short generic name collided
# with something the shell already owned.
#
# So the prefix is applied here, and a new variable is named right because there is no other
# way to ask for one.

Set-StrictMode -Version Latest

# One of ours, by its BARE name: Get-DegctEnv ROW_SECONDS -Default 600
function Get-DegctEnv {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true, Position = 0)][string] $Name,
        [Parameter(Position = 1)] $Default = $null,
        # A variable somebody else owns - PATH, CARGO_TARGET_DIR, NUMBER_OF_PROCESSORS - read
        # under its own name. A switch rather than a second function so a call site says which
        # of the two it means.
        [switch] $Foreign
    )

    $key = if ($Foreign) { $Name } else { ConvertTo-DegctEnvName $Name }
    $value = [Environment]::GetEnvironmentVariable($key)
    if ($null -eq $value -or $value -eq '') { return $Default }
    return $value
}

# Whether one of ours is set at all, whatever it is set to.
#
# The shape a flag takes here: several measurements switch on PRESENCE rather than value, so
# DEGCT_NOLIMIT=1 and DEGCT_NOLIMIT= mean the same and neither has to be parsed.
function Test-DegctEnv {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true, Position = 0)][string] $Name)

    $key = ConvertTo-DegctEnvName $Name
    return $null -ne [Environment]::GetEnvironmentVariable($key)
}

# Sets one of ours for this process and the children it starts.
function Set-DegctEnv {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true, Position = 0)][string] $Name,
        [Parameter(Mandatory = $true, Position = 1)][string] $Value
    )

    $key = ConvertTo-DegctEnvName $Name
    [Environment]::SetEnvironmentVariable($key, $Value)
}

# The full name of one of ours. Idempotent, because callers build names from both halves - a
# bare one they were given and a full one read back out of a message - and
# DEGCT_DEGCT_CONVERSATION would be unset, silently, and read as a default.
function ConvertTo-DegctEnvName {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true, Position = 0)][string] $Name)

    if ($Name.StartsWith('DEGCT_')) { return $Name }
    return "DEGCT_$Name"
}

Export-ModuleMember -Function Get-DegctEnv, Test-DegctEnv, Set-DegctEnv, ConvertTo-DegctEnvName
