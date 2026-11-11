#pragma once

enum class PaddleDir { NONE, UP, DOWN };

// 32x16 핑퐁 게임 엔진 클래스 (선언부)
class PongGame {
public:
    static const int WIDTH = 32;
    static const int HEIGHT = 16;
    static const int PADDLE_HEIGHT = 4;
    static const int WIN_SCORE = 5;

private:
    float ballX, ballY;
    float ballVX, ballVY;

    int paddle1Y; // 1P (왼쪽)
    int paddle2Y; // 2P 또는 AI (오른쪽)

    int score1, score2;
    int rallyScore;
    bool twoPlayerMode;
    bool gameOver;
    bool frameBuffer[HEIGHT][WIDTH];

public:
    PongGame(bool isTwoPlayer = false);

    void resetGame();
    void setTwoPlayerMode(bool enable);
    void resetBall(int serveDir);
    void movePaddle1(PaddleDir dir);
    void movePaddle2(PaddleDir dir);
    void update();
    void updateFrameBuffer();

    bool getPixel(int x, int y) const;
    bool isGameOver() const;
    bool isTwoPlayer() const;
    int getScore1() const;
    int getScore2() const;
    int getPlayerTotalScore() const;
    int getBallX() const;
    int getBallY() const;
};
