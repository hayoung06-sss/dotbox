#include <iostream>
#include <chrono>
#include <thread>
#include "ScoreManager.h"
#include "SnakeGame.h"
#include "PongGame.h"

#ifdef _WIN32
#include <conio.h>
#endif

void renderSnake(const SnakeGame& game, int highScore) {
    std::cout << "\033[H";
    std::cout << "=== DotBox 32x16 Snake [점수: " << game.getScore()
              << " | 로컬 최고 기록(EEPROM): " << highScore << "] ===\n";

    for (int x = 0; x < SnakeGame::WIDTH + 2; ++x) std::cout << "# ";
    std::cout << "\n";

    for (int y = 0; y < SnakeGame::HEIGHT; ++y) {
        std::cout << "# ";
        for (int x = 0; x < SnakeGame::WIDTH; ++x) {
            if (Point{x, y} == game.getHead()) std::cout << "@ ";
            else if (Point{x, y} == game.getFood()) std::cout << "* ";
            else if (game.getPixel(x, y)) std::cout << "O ";
            else std::cout << ". ";
        }
        std::cout << "#\n";
    }

    for (int x = 0; x < SnakeGame::WIDTH + 2; ++x) std::cout << "# ";
    std::cout << "\n[조작] W/A/S/D: 방향 이동 | Q: 메뉴로 나가기\n";
}

void renderPong(const PongGame& game, int highScore) {
    std::cout << "\033[H";
    std::cout << "=== DotBox 32x16 Pong [" << (game.isTwoPlayer() ? "2P 대전" : "1P vs AI")
              << "] | 1P 점수: " << game.getPlayerTotalScore()
              << " | 로컬 최고 기록(EEPROM): " << highScore << " ===\n";
    std::cout << "    [ 1P 세트 득점: " << game.getScore1()
              << " ]          [ 2P 세트 득점: " << game.getScore2() << " ]\n";

    for (int x = 0; x < PongGame::WIDTH + 2; ++x) std::cout << "# ";
    std::cout << "\n";

    for (int y = 0; y < PongGame::HEIGHT; ++y) {
        std::cout << "# ";
        for (int x = 0; x < PongGame::WIDTH; ++x) {
            if (x == game.getBallX() && y == game.getBallY()) std::cout << "O ";
            else if (game.getPixel(x, y)) std::cout << "| ";
            else if (x == PongGame::WIDTH / 2 && y % 2 == 0) std::cout << ": ";
            else std::cout << ". ";
        }
        std::cout << "#\n";
    }

    for (int x = 0; x < PongGame::WIDTH + 2; ++x) std::cout << "# ";
    std::cout << "\n[조작] 1P: W/S | 2P: I/K | M: 1인/2인 전환 | Q: 메뉴로 나가기\n";
}

void runSnake(ScoreManager& scoreMgr) {
    SnakeGame game;
    std::cout << "\033[2J";

    while (!game.isGameOver()) {
#ifdef _WIN32
        if (_kbhit()) {
            char ch = _getch();
            if (ch == 'w' || ch == 'W') game.setDirection(Direction::UP);
            else if (ch == 's' || ch == 'S') game.setDirection(Direction::DOWN);
            else if (ch == 'a' || ch == 'A') game.setDirection(Direction::LEFT);
            else if (ch == 'd' || ch == 'D') game.setDirection(Direction::RIGHT);
            else if (ch == 'q' || ch == 'Q') break;
        }
#endif
        game.update();
        renderSnake(game, scoreMgr.getHighScore("snake"));
        std::this_thread::sleep_for(std::chrono::milliseconds(game.getStepDelayMs()));
    }

    bool isNew = scoreMgr.updateScore("snake", game.getScore());
    std::cout << "\n[Game Over] 최종 점수: " << game.getScore() << "점\n";
    if (isNew) {
        std::cout << ">>> 축하합니다! 로컬 최고 기록(EEPROM)이 갱신되었습니다! <<<\n";
    }
    std::cout << "최고 점수: " << scoreMgr.getHighScore("snake") << "점\n";
}

void runPong(ScoreManager& scoreMgr) {
    PongGame game(false);
    std::cout << "\033[2J";

    while (!game.isGameOver()) {
        PaddleDir d1 = PaddleDir::NONE;
        PaddleDir d2 = PaddleDir::NONE;
#ifdef _WIN32
        while (_kbhit()) {
            char ch = _getch();
            if (ch == 'w' || ch == 'W') d1 = PaddleDir::UP;
            else if (ch == 's' || ch == 'S') d1 = PaddleDir::DOWN;
            else if (ch == 'i' || ch == 'I') d2 = PaddleDir::UP;
            else if (ch == 'k' || ch == 'K') d2 = PaddleDir::DOWN;
            else if (ch == 'm' || ch == 'M') game.setTwoPlayerMode(!game.isTwoPlayer());
            else if (ch == 'q' || ch == 'Q') return;
        }
#endif
        game.movePaddle1(d1);
        game.movePaddle2(d2);
        game.update();
        renderPong(game, scoreMgr.getHighScore("pong"));
        std::this_thread::sleep_for(std::chrono::milliseconds(80));
    }

    bool isNew = scoreMgr.updateScore("pong", game.getPlayerTotalScore());
    std::cout << "\n[Game Over] " << (game.getScore1() > game.getScore2() ? "1P 승리!" : "2P(AI) 승리!") << "\n";
    std::cout << "1P 획득 점수: " << game.getPlayerTotalScore() << "점\n";
    if (isNew) {
        std::cout << ">>> 축하합니다! 로컬 최고 기록(EEPROM)이 갱신되었습니다! <<<\n";
    }
    std::cout << "최고 점수: " << scoreMgr.getHighScore("pong") << "점\n";
}

int main() {
    ScoreManager scoreMgr("eeprom_highscore.dat");

    while (true) {
        std::cout << "\n==============================================\n";
        std::cout << "   DotBox 32x16 아케이드 콘솔 (정석 분리 구조)  \n";
        std::cout << "==============================================\n";
        std::cout << " [로컬 최고 기록 (EEPROM 유지)]\n";
        std::cout << "  - 뱀 게임 최고 점수 : " << scoreMgr.getHighScore("snake") << " 점\n";
        std::cout << "  - 핑퐁 최고 점수    : " << scoreMgr.getHighScore("pong") << " 점\n";
        std::cout << "----------------------------------------------\n";
        std::cout << "  1. 뱀 게임 (Snake) 시작\n";
        std::cout << "  2. 핑퐁 게임 (Pong) 시작\n";
        std::cout << "  0. 종료\n";
        std::cout << "==============================================\n";
        std::cout << "선택 > ";

        int choice = 0;
        if (!(std::cin >> choice) || choice == 0) break;

        if (choice == 1) runSnake(scoreMgr);
        else if (choice == 2) runPong(scoreMgr);
    }

    return 0;
}
