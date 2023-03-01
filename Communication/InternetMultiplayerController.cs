using Core;
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using Microsoft.AspNetCore.Mvc;
using Newtonsoft.Json;
using System.Xml;

namespace Communication;

public class InternetMultiPlayerInfoController : IMultiPlayerInfoViewController, IMultiPlayerInfoPlayerInfoInputController
{
    private ReaderWriterLockSlim _lock = new ReaderWriterLockSlim();
    private int _wantedUniqueIDs;
    private List<int> _availableUniqueIDs = new List<int>();

    public List<(PlayerInfo, string)> FetchRequestedGameLobbiesWithLobbyNameAndPlayerInfo()
    {
        VerifyLock();
        var newGames = new List<(PlayerInfo, string)>();
        // TODO: Return all the new wanted games
        return newGames;
    }

    public List<Input> FetchPlayerInfoInputs(int gameID)
    {
        VerifyLock();
        var PlayerInfoInputs = new List<Input>();
        // TODO: Return all the inputs from the game with the given gameID
        return PlayerInfoInputs;
    }

    public void SendNewGameStateInfoToPlayerInfos(GameStateInfo state)
    {
        VerifyLock();
        // TODO: Send game state to the PlayerInfos in the game connected to the game state input
    }

    public void Lock()
    {
        _lock.EnterWriteLock();
    }

    public void ReleaseLock()
    {
        _lock.ExitWriteLock();
    }

    public void VerifyLock()
    {
        if (!_lock.IsWriteLockHeld) throw new InvalidOperationException("Before making any calls to this object " +
                                                                        "it needs to be locked unsing Lock() and " +
                                                                        "needs to be released once done!");
    }

    public void HandleGeneratedUniqueIDs(List<int> uniqueIDs)
    {
        _availableUniqueIDs.AddRange(uniqueIDs);
        _wantedUniqueIDs -= uniqueIDs.Count;
    }

    public int FetchWantedAmountOfUniqueIDs()
    {
        return _wantedUniqueIDs;
    }
    public void NotifyWantID()
    {
        VerifyLock();
        _wantedUniqueIDs++;
    }
    public (bool, int) FetchUniqueID()
    {
        VerifyLock();
        if (_availableUniqueIDs.Count <= 0) return (false, 0);
        int id = _availableUniqueIDs.First();
        _availableUniqueIDs.Remove(id);
        return (true, id);
    }
}