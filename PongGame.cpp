#include "PongGame.h"
#include <cmath>
#include <algorithm>

PongGame::PongGame(bool isTwoPlayer) : twoPlayerMode(isTwoPlayer) {
    resetGame();
}

void PongGame::resetGame() {
    score1 = 0;
    score2 = 0;
    rallyScore = 0;
    gameOver = false;
    paddle1Y = HEIGHT / 2 - PADDLE_HEIGHT / 2;
    paddle2Y = HEIGHT / 2 - PADDLE_HEIGHT / 2;
    resetBall(1);
    updateFrameBuffer();
}

void PongGame::setTwoPlayerMode(bool enable) {
    twoPlayerMode = enable;
}

void PongGame::resetBall(int serveDir) {
    ballX = WIDTH / 2.0f;
    ballY = HEIGHT / 2.0f;
    ballVX = 0.75f * serveDir;
    ballVY = 0.25f;
}

void PongGame::movePaddle1(PaddleDir dir) {
    if (dir == PaddleDir::UP && paddle1Y > 0) paddle1Y--;
    else if (dir == PaddleDir::DOWN && paddle1Y + PADDLE_HEIGHT < HEIGHT) paddle1Y++;
}

void PongGame::movePaddle2(PaddleDir dir) {
    if (!twoPlayerMode) return;
    if (dir == PaddleDir::UP && paddle2Y > 0) paddle2Y--;
    else if (dir == PaddleDir::DOWN && paddle2Y + PADDLE_HEIGHT < HEIGHT) paddle2Y++;
}

void PongGame::update() {
    if (gameOver) return;

    // 1인용 모드 시 AI 패들 제어
    if (!twoPlayerMode && ballVX > 0) {
        float paddleCenter = paddle2Y + (PADDLE_HEIGHT - 1) / 2.0f;
        if (ballY < paddleCenter - 0.6f && paddle2Y > 0) {
            paddle2Y--;
        } else if (ballY > paddleCenter + 0.6f && paddle2Y + PADDLE_HEIGHT < HEIGHT) {
            paddle2Y++;
        }
    }

    ballX += ballVX;
    ballY += ballVY;

    // 상하 벽면 반사
    if (ballY <= 0.0f) {
        ballY = 0.0f;
        ballVY = -ballVY;
    } else if (ballY >= HEIGHT - 1.0f) {
        ballY = HEIGHT - 1.0f;
        ballVY = -ballVY;
    }

    int bX = static_cast<int>(std::round(ballX));
    int bY = static_cast<int>(std::round(ballY));

    // 왼쪽 패들(1P, X=1) 충돌 및 반사각 계산
    if (ballVX < 0 && bX <= 1 && bX >= 0) {
        if (bY >= paddle1Y && bY < paddle1Y + PADDLE_HEIGHT) {
            ballX = 1.0f;
            ballVX = -ballVX;
            float center = paddle1Y + (PADDLE_HEIGHT - 1) / 2.0f;
            float hitOffset = (ballY - center) / (PADDLE_HEIGHT / 2.0f);
            ballVY = std::clamp(hitOffset * 0.75f, -0.85f, 0.85f);
            rallyScore += 5;
        }
    }

    // 오른쪽 패들(2P/AI, X=WIDTH-2) 충돌 및 반사각 계산
    if (ballVX > 0 && bX >= WIDTH - 2 && bX < WIDTH) {
        if (bY >= paddle2Y && bY < paddle2Y + PADDLE_HEIGHT) {
            ballX = WIDTH - 2.0f;
            ballVX = -ballVX;
            float center = paddle2Y + (PADDLE_HEIGHT - 1) / 2.0f;
            float hitOffset = (ballY - center) / (PADDLE_HEIGHT / 2.0f);
            ballVY = std::clamp(hitOffset * 0.75f, -0.85f, 0.85f);
        }
    }

    // 좌우 골라인 통과 (득점 판정)
    if (ballX < 0.0f) {
        score2++;
        if (score2 >= WIN_SCORE) gameOver = true;
        else resetBall(1);
    } else if (ballX >= WIDTH) {
        score1++;
        rallyScore += 20;
        if (score1 >= WIN_SCORE) gameOver = true;
        else resetBall(-1);
    }

    updateFrameBuffer();
}

void PongGame::updateFrameBuffer() {
    for (int y = 0; y < HEIGHT; ++y) {
        for (int x = 0; x < WIDTH; ++x) {
            frameBuffer[y][x] = false;
        }
    }

    for (int i = 0; i < PADDLE_HEIGHT; ++i) {
        frameBuffer[paddle1Y + i][1] = true;
        frameBuffer[paddle2Y + i][WIDTH - 2] = true;
    }

    int bX = static_cast<int>(std::round(ballX));
    int bY = static_cast<int>(std::round(ballY));
    if (bX >= 0 && bX < WIDTH && bY >= 0 && bY < HEIGHT) {
        frameBuffer[bY][bX] = true;
    }
}

bool PongGame::getPixel(int x, int y) const { return frameBuffer[y][x]; }
bool PongGame::isGameOver() const { return gameOver; }
bool PongGame::isTwoPlayer() const { return twoPlayerMode; }
int PongGame::getScore1() const { return score1; }
int PongGame::getScore2() const { return score2; }
int PongGame::getPlayerTotalScore() const { return rallyScore; }
int PongGame::getBallX() const { return static_cast<int>(std::round(ballX)); }
int PongGame::getBallY() const { return static_cast<int>(std::round(ballY)); }
