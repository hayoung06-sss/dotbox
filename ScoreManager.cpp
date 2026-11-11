#include "ScoreManager.h"
#include <fstream>

ScoreManager::ScoreManager(const std::string& path)
    : saveFilePath(path) {
    loadFromEEPROM();
}

void ScoreManager::loadFromEEPROM() {
    highScores.clear();
    std::ifstream in(saveFilePath);
    if (in.is_open()) {
        std::string gameId;
        int score;
        while (in >> gameId >> score) {
            highScores[gameId] = score;
        }
        in.close();
    }
}

void ScoreManager::saveToEEPROM() const {
    std::ofstream out(saveFilePath, std::ios::trunc);
    if (out.is_open()) {
        for (const auto& pair : highScores) {
            out << pair.first << " " << pair.second << "\n";
        }
        out.close();
    }
}

bool ScoreManager::updateScore(const std::string& gameId, int currentScore) {
    if (currentScore > highScores[gameId]) {
        highScores[gameId] = currentScore;
        saveToEEPROM();
        return true;
    }
    return false;
}

int ScoreManager::getHighScore(const std::string& gameId) const {
    auto it = highScores.find(gameId);
    if (it != highScores.end()) {
        return it->second;
    }
    return 0;
}
