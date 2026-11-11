#pragma once
#include <string>
#include <map>

// 범용 로컬 최고 점수 관리 클래스 (선언부)
// 새로운 게임이 추가되어도 이 클래스를 수정할 필요 없이 문자열 ID만으로 점수를 관리합니다.
class ScoreManager {
private:
    std::string saveFilePath;
    std::map<std::string, int> highScores;

public:
    ScoreManager(const std::string& path = "eeprom_highscore.dat");

    void loadFromEEPROM();
    void saveToEEPROM() const;

    bool updateScore(const std::string& gameId, int currentScore);
    int getHighScore(const std::string& gameId) const;
};
